# `hutt server` — mu server proxy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make `hutt server` a drop-in replacement for `mu server` that proxies commands through a running hutt instance's mu server via bidirectional IPC, with fallback to standalone mu when hutt isn't available.

**Architecture:** Three layers — (1) bidirectional IPC in `links.rs` (request/response with `IpcResponse`), (2) `MuCommand` handling in the event loop that forwards raw S-expressions to the correct `MuClient`, (3) `hutt server` CLI shim in `main.rs` that speaks mu wire protocol on stdin/stdout and translates to/from IPC.

**Tech Stack:** Rust, tokio, serde_json, Unix domain sockets, mu S-expression wire protocol (0xfe/0xff framing)

---

### Task 1: Add `IpcResponse` enum and `encode_frame` helper

**Files:**
- Modify: `src/links.rs` (add `IpcResponse` enum)
- Modify: `src/mu_sexp.rs` (add `encode_frame` function)

**Step 1: Add `encode_frame` to mu_sexp.rs with a test**

Add to `src/mu_sexp.rs` after the `read_frame` function:

```rust
/// Encode an S-expression string into mu's wire frame format.
/// Format: \xfe<hex-length>\xff<sexp-bytes>
pub fn encode_frame(sexp: &str) -> Vec<u8> {
    let len_hex = format!("{:x}", sexp.len());
    let mut buf = Vec::with_capacity(2 + len_hex.len() + sexp.len());
    buf.push(0xfe);
    buf.extend_from_slice(len_hex.as_bytes());
    buf.push(0xff);
    buf.extend_from_slice(sexp.as_bytes());
    buf
}
```

Add test in the `#[cfg(test)]` module:

```rust
#[test]
fn test_encode_frame_roundtrip() {
    let sexp = "(:pong \"mu\")";
    let encoded = encode_frame(sexp);
    let (value, consumed) = read_frame(&encoded).unwrap().unwrap();
    assert_eq!(consumed, encoded.len());
    assert!(is_pong(&value));
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test test_encode_frame_roundtrip`

**Step 3: Add `IpcResponse` enum to links.rs**

Add after the `IpcCommand` enum in `src/links.rs`:

```rust
/// Response sent back to IPC clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    Ok,
    Error { message: String },
    MuFrames { frames: Vec<String> },
}
```

**Step 4: Run `cargo check`**

**Step 5: Commit**

```bash
git add src/links.rs src/mu_sexp.rs
git commit -m "Add IpcResponse enum and encode_frame helper"
```

---

### Task 2: Add `MuCommand` variant to `IpcCommand`

**Files:**
- Modify: `src/links.rs` (add variant to `IpcCommand`)

**Step 1: Add the variant**

Add to the `IpcCommand` enum:

```rust
MuCommand {
    sexp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muhome: Option<String>,
},
```

**Step 2: Add test for MuCommand serialization**

In the existing test module in `links.rs`, add:

```rust
#[test]
fn test_mu_command_serde() {
    let cmd = IpcCommand::MuCommand {
        sexp: "(find :query \"flag:unread\" :sortfield :date :maxnum 500 :threads t :descending t)".to_string(),
        account: Some("fil".to_string()),
        muhome: None,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: IpcCommand = serde_json::from_str(&json).unwrap();
    match parsed {
        IpcCommand::MuCommand { sexp, account, muhome } => {
            assert!(sexp.contains("flag:unread"));
            assert_eq!(account.as_deref(), Some("fil"));
            assert!(muhome.is_none());
        }
        _ => panic!("expected MuCommand"),
    }
}
```

**Step 3: Run tests**

Run: `cargo test test_mu_command_serde`

**Step 4: Commit**

```bash
git add src/links.rs
git commit -m "Add MuCommand variant to IpcCommand"
```

---

### Task 3: Make IPC bidirectional (accept returns stream, send reads response)

**Files:**
- Modify: `src/links.rs` (change `accept` to return stream, change `send_ipc_command` to read response)
- Modify: `src/tui/mod.rs` (IPC listener task and handler now write responses back)

**Step 1: Change `IpcListener::accept` to return the command AND the stream**

Replace the `accept` method in `src/links.rs`:

```rust
/// Accept a single connection, read a JSON-encoded `IpcCommand`,
/// and return it along with the stream for sending a response.
pub async fn accept(&self) -> Result<(IpcCommand, UnixStream)> {
    let (mut stream, _addr) = self
        .listener
        .accept()
        .await
        .context("accepting IPC connection")?;

    let mut buf = Vec::with_capacity(4096);
    // Read until the client shuts down their write side
    loop {
        let mut tmp = [0u8; 4096];
        let n = stream.read(&mut tmp).await.context("reading IPC command")?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }

    let cmd: IpcCommand =
        serde_json::from_slice(&buf).context("deserializing IPC command")?;
    Ok((cmd, stream))
}
```

**Step 2: Add `send_response` helper function**

Add to `src/links.rs`:

```rust
/// Write a JSON-encoded `IpcResponse` to a stream.
pub async fn send_response(stream: &mut UnixStream, resp: &IpcResponse) -> Result<()> {
    let json = serde_json::to_vec(resp).context("serializing IPC response")?;
    stream.write_all(&json).await.context("writing IPC response")?;
    stream.shutdown().await.context("shutting down IPC response stream")?;
    Ok(())
}
```

**Step 3: Update `send_ipc_command` to read response**

Replace in `src/links.rs`:

```rust
/// Client side: connect to the running hutt instance, send a command,
/// and read back the response.
pub async fn send_ipc_command(cmd: &IpcCommand) -> Result<IpcResponse> {
    let path = socket_path();
    if !path.exists() {
        bail!(
            "no hutt instance running (socket {} not found)",
            path.display()
        );
    }

    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;

    let json = serde_json::to_vec(cmd).context("serializing IPC command")?;
    stream
        .write_all(&json)
        .await
        .context("writing IPC command")?;
    // Shut down write side so the server knows we're done sending
    stream.shutdown().await.context("shutting down write side")?;

    // Read the response
    let mut resp_buf = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut resp_buf)
        .await
        .context("reading IPC response")?;

    let resp: IpcResponse =
        serde_json::from_slice(&resp_buf).context("deserializing IPC response")?;
    Ok(resp)
}
```

**Step 4: Update IPC listener task in `src/tui/mod.rs`**

Change the channel type from `IpcCommand` to `(IpcCommand, tokio::sync::oneshot::Sender<IpcResponse>)`. The listener task accepts the connection, reads the command, sends it through the channel with a oneshot sender, waits for the response, and writes it back.

Change the channel:
```rust
let (ipc_tx, mut ipc_rx) = tokio::sync::mpsc::unbounded_channel::<(IpcCommand, tokio::sync::oneshot::Sender<links::IpcResponse>)>();
```

Change the listener task:
```rust
Some(tokio::spawn(async move {
    debug_log!("IPC listener started");
    loop {
        match listener.accept().await {
            Ok((cmd, mut stream)) => {
                debug_log!("IPC accepted: {:?}", cmd);
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                if tx.send((cmd, resp_tx)).is_err() {
                    debug_log!("IPC channel closed, exiting");
                    break;
                }
                // Wait for the response from the event loop and send it back
                tokio::spawn(async move {
                    let resp = match resp_rx.await {
                        Ok(resp) => resp,
                        Err(_) => links::IpcResponse::Error {
                            message: "internal: response channel dropped".to_string(),
                        },
                    };
                    if let Err(e) = links::send_response(&mut stream, &resp).await {
                        debug_log!("IPC response send error: {}", e);
                    }
                });
            }
            Err(e) => {
                debug_log!("IPC accept error: {}", e);
                continue;
            }
        }
    }
}))
```

**Step 5: Update `handle_ipc_command` to return `IpcResponse`**

Change signature in `src/tui/mod.rs`:
```rust
async fn handle_ipc_command(&mut self, cmd: IpcCommand) -> IpcResponse {
```

Wrap existing body: existing match arms return `IpcResponse::Ok` on success, `IpcResponse::Error { message }` on error. The `MuCommand` arm will be added in the next task.

**Step 6: Update all call sites in the event loop**

The drain loop and select loop both call `handle_ipc_command`. Update them to destructure the tuple, call the handler, and send the response through the oneshot channel:

```rust
// Drain loop:
while let Ok((cmd, resp_tx)) = ipc_rx.try_recv() {
    debug_log!("IPC drain: {:?}", cmd);
    let resp = app.handle_ipc_command(cmd).await;
    let _ = resp_tx.send(resp);
}

// Select loop:
cmd = ipc_rx.recv() => {
    if let Some((cmd, resp_tx)) = cmd {
        debug_log!("IPC select: {:?}", cmd);
        let resp = app.handle_ipc_command(cmd).await;
        let _ = resp_tx.send(resp);
    }
    continue;
}
```

**Step 7: Run `cargo check` and `cargo test`**

**Step 8: Commit**

```bash
git add src/links.rs src/tui/mod.rs
git commit -m "Make IPC bidirectional: commands now return IpcResponse

IpcListener::accept returns the stream alongside the command.
send_ipc_command reads the response. Event loop sends IpcResponse
back through a oneshot channel."
```

---

### Task 4: Update `run_remote` to handle `IpcResponse`

**Files:**
- Modify: `src/main.rs` (update `run_remote` to check response)

**Step 1: Update `run_remote`**

Change the end of the function from:
```rust
links::send_ipc_command(&cmd).await
```
to:
```rust
let resp = links::send_ipc_command(&cmd).await?;
match resp {
    links::IpcResponse::Ok => Ok(()),
    links::IpcResponse::Error { message } => {
        bail!("hutt: {}", message);
    }
    links::IpcResponse::MuFrames { .. } => {
        // Shouldn't happen for regular remote commands
        Ok(())
    }
}
```

**Step 2: Run `cargo check`**

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "Update run_remote to handle IpcResponse errors"
```

---

### Task 5: Handle `MuCommand` in the event loop

**Files:**
- Modify: `src/tui/mod.rs` (add MuCommand handling to `handle_ipc_command`)
- Modify: `src/mu_client.rs` (add `send_raw` and `read_raw_frames` methods)

**Step 1: Add raw command methods to `MuClient`**

Add to `src/mu_client.rs`:

```rust
/// Send a raw S-expression command and collect all response frames
/// as strings until a terminal frame is reached.
///
/// Terminal frames are: :found, :pong, :update (for move), :index (for index),
/// :contacts, or :error.
pub async fn send_raw(&mut self, sexp: &str) -> Result<Vec<String>> {
    self.send(sexp).await?;
    let mut frames = Vec::new();
    loop {
        let value = self.reader.next_frame().await?;
        let frame_str = format!("{}", value);
        let is_terminal = mu_sexp::is_found(&value).is_some()
            || mu_sexp::is_pong(&value)
            || mu_sexp::is_error(&value).is_some()
            || mu_sexp::is_update(&value)
            || mu_sexp::plist_get(&value, "index").is_some()
            || mu_sexp::plist_get(&value, "contacts").is_some()
            || mu_sexp::plist_get(&value, "info").is_some();
        frames.push(frame_str);
        if is_terminal {
            break;
        }
    }
    Ok(frames)
}
```

**Step 2: Add `MuCommand` arm to `handle_ipc_command` in `src/tui/mod.rs`**

Add before the closing brace of the match:

```rust
IpcCommand::MuCommand { sexp, account, muhome } => {
    // Resolve which mu server to use
    let target_idx = self.resolve_mu_target(account.as_deref(), muhome.as_deref());
    match target_idx {
        Some(idx) => {
            // If indexing on the target server, reject
            if idx == self.active_account && self.indexing {
                return IpcResponse::Error {
                    message: "mu server is busy (indexing)".to_string(),
                };
            }
            let mu = if idx == self.active_account {
                &mut self.mu
            } else {
                match self.background_mu.get_mut(&idx) {
                    Some(mu) => mu,
                    None => return IpcResponse::Error {
                        message: format!("no mu server running for account index {}", idx),
                    },
                }
            };
            match mu.send_raw(&sexp).await {
                Ok(frames) => IpcResponse::MuFrames { frames },
                Err(e) => IpcResponse::Error {
                    message: format!("mu command error: {}", e),
                },
            }
        }
        None => IpcResponse::Error {
            message: "no matching account for muhome/account".to_string(),
        },
    }
}
```

**Step 3: Add `resolve_mu_target` method to `App`**

```rust
/// Resolve an account index from optional account name and/or muhome path.
/// muhome takes precedence over account name.
/// Returns None if specified but not found; Some(active_account) if neither specified.
fn resolve_mu_target(&self, account: Option<&str>, muhome: Option<&str>) -> Option<usize> {
    if let Some(mh) = muhome {
        // Match muhome against all accounts
        for (idx, _acct) in self.config.accounts.iter().enumerate() {
            let effective = self.config.effective_muhome(idx);
            if effective.as_deref() == Some(mh) {
                return Some(idx);
            }
        }
        // Also check if muhome is None (system default) — match if mh is the default mu path
        return None;
    }
    if let Some(name) = account {
        return self.config.accounts.iter().position(|a| a.name == *name);
    }
    Some(self.active_account)
}
```

**Step 4: Run `cargo check` and `cargo test`**

**Step 5: Invalidate folder cache after mu commands that modify state**

After `send_raw` succeeds, check if the command looks like a mutation (move, index):
```rust
// After Ok(frames) in the MuCommand handler:
if sexp.starts_with("(move") || sexp.starts_with("(index") {
    self.invalidate_folder_cache();
}
```

**Step 6: Commit**

```bash
git add src/mu_client.rs src/tui/mod.rs
git commit -m "Handle MuCommand: forward raw S-expressions to mu server

Resolves target account by --muhome or --account name.
Collects all response frames and returns as MuFrames.
Invalidates folder cache after mutations."
```

---

### Task 6: Implement `hutt server` CLI subcommand

**Files:**
- Modify: `src/main.rs` (add `server` subcommand parsing and `run_server` function)

**Step 1: Add `server` subcommand parsing to main**

In the CLI match, after `"remote" | "r"`, add:

```rust
"server" => {
    return run_server(&args[i + 1..]).await;
}
```

**Step 2: Add `print_server_help` function**

```rust
fn print_server_help() {
    eprintln!(
        "hutt server — mu server proxy (drop-in replacement for mu server)

USAGE:
    hutt server [OPTIONS]

OPTIONS:
    -h, --help              Show help information
    --commands              List available commands
    --eval TEXT             Evaluate mu server expression
    --allow-temp-file       Allow for the temp-file optimization (accepted, ignored)
    --muhome <dir>          Select account by muhome path (or fall back to standalone mu)
    --account <name>        Select account by name

When hutt is running, commands are proxied through its mu server.
When hutt is not running (or --muhome doesn't match), falls back to
running mu server directly."
    );
}
```

**Step 3: Add `run_server` function**

```rust
async fn run_server(args: &[String]) -> Result<()> {
    let mut muhome: Option<String> = None;
    let mut account: Option<String> = None;
    let mut eval: Option<String> = None;
    let mut allow_temp_file = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_server_help();
                return Ok(());
            }
            "--commands" => {
                println!("commands: compose contacts find index move ping quit");
                return Ok(());
            }
            "--eval" => {
                i += 1;
                eval = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--eval requires a TEXT argument"))?
                        .clone(),
                );
            }
            "--allow-temp-file" => {
                allow_temp_file = true;
            }
            "--muhome" => {
                i += 1;
                muhome = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--muhome requires a path"))?
                        .clone(),
                );
            }
            "--account" | "-a" => {
                i += 1;
                account = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--account requires a name"))?
                        .clone(),
                );
            }
            other => {
                eprintln!("Unknown option: {}", other);
                print_server_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Try connecting to hutt first
    let hutt_available = links::socket_path().exists();

    if let Some(ref eval_sexp) = eval {
        if hutt_available {
            return run_server_eval(eval_sexp, account, muhome).await;
        }
        return run_mu_fallback(&args).await;
    }

    if hutt_available {
        run_server_interactive(account, muhome).await
    } else {
        run_mu_fallback(&args).await
    }
}
```

**Step 4: Add `run_server_eval` function**

```rust
/// Send a single S-expression via IPC and print the response frames.
async fn run_server_eval(sexp: &str, account: Option<String>, muhome: Option<String>) -> Result<()> {
    let cmd = links::IpcCommand::MuCommand {
        sexp: sexp.to_string(),
        account,
        muhome,
    };
    let resp = links::send_ipc_command(&cmd).await?;
    match resp {
        links::IpcResponse::MuFrames { frames } => {
            for frame in frames {
                // Write in mu wire format to stdout
                let encoded = mu_sexp::encode_frame(&frame);
                use std::io::Write;
                std::io::stdout().write_all(&encoded)?;
                std::io::stdout().flush()?;
            }
            Ok(())
        }
        links::IpcResponse::Error { message } => bail!("{}", message),
        links::IpcResponse::Ok => Ok(()),
    }
}
```

**Step 5: Add `run_server_interactive` function**

```rust
/// Interactive mode: read S-expressions from stdin, proxy through hutt,
/// write responses to stdout in mu wire format.
async fn run_server_interactive(account: Option<String>, muhome: Option<String>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    // Emit a synthetic pong greeting like mu server does
    let greeting = mu_sexp::encode_frame("(:pong \"hutt-proxy\" :props (:version \"hutt\"))");
    {
        use std::io::Write;
        std::io::stdout().write_all(&greeting)?;
        std::io::stdout().flush()?;
    }

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let sexp = line.trim().to_string();
        if sexp.is_empty() {
            continue;
        }

        // Handle quit locally
        if sexp == "(quit)" {
            break;
        }

        let cmd = links::IpcCommand::MuCommand {
            sexp,
            account: account.clone(),
            muhome: muhome.clone(),
        };

        match links::send_ipc_command(&cmd).await {
            Ok(resp) => match resp {
                links::IpcResponse::MuFrames { frames } => {
                    use std::io::Write;
                    for frame in frames {
                        let encoded = mu_sexp::encode_frame(&frame);
                        std::io::stdout().write_all(&encoded)?;
                    }
                    std::io::stdout().flush()?;
                }
                links::IpcResponse::Error { message } => {
                    use std::io::Write;
                    let err_sexp = format!("(:error :message \"{}\")", message.replace('"', "\\\""));
                    let encoded = mu_sexp::encode_frame(&err_sexp);
                    std::io::stdout().write_all(&encoded)?;
                    std::io::stdout().flush()?;
                }
                links::IpcResponse::Ok => {}
            },
            Err(e) => {
                // Connection to hutt lost — fall back to error
                use std::io::Write;
                let err_sexp = format!("(:error :message \"{}\")", e.to_string().replace('"', "\\\""));
                let encoded = mu_sexp::encode_frame(&err_sexp);
                std::io::stdout().write_all(&encoded)?;
                std::io::stdout().flush()?;
            }
        }
    }

    Ok(())
}
```

**Step 6: Add `run_mu_fallback` function**

```rust
/// Fall back to running mu server directly with the original args.
async fn run_mu_fallback(args: &[String]) -> Result<()> {
    // Filter out --account (not a mu flag), keep everything else
    let mut mu_args = vec!["server".to_string()];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--account" | "-a" => { i += 1; } // skip --account and its value
            other => mu_args.push(other.to_string()),
        }
        i += 1;
    }

    let status = tokio::process::Command::new("mu")
        .args(&mu_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .context("failed to run mu server")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}
```

**Step 7: Make `socket_path` public in links.rs**

Change `fn socket_path()` to `pub fn socket_path()` in `src/links.rs`.

**Step 8: Run `cargo check` and `cargo clippy`**

**Step 9: Commit**

```bash
git add src/main.rs src/links.rs
git commit -m "Add 'hutt server' subcommand: drop-in mu server replacement

Interactive and --eval modes. Proxies through running hutt when
available, falls back to standalone mu server otherwise. Matches
mu server CLI options (--muhome, --eval, --commands, --allow-temp-file)."
```

---

### Task 7: Update help text, docs, and final polish

**Files:**
- Modify: `src/main.rs` (update help text)
- Modify: `CLAUDE.md` (document new feature)

**Step 1: Update `print_help` in main.rs**

Add `hutt server` to the USAGE section:

```
    hutt server [OPTIONS]            Run as mu server proxy (drop-in for mu server)
```

And add to the help body:

```
SERVER OPTIONS (drop-in replacement for mu server):
    --muhome <dir>              Select account by muhome path
    --account <name>            Select account by name
    --eval TEXT                 Evaluate a single mu server expression
    --commands                  List available mu server commands
    --allow-temp-file           Accepted for compatibility (ignored)
```

**Step 2: Run all tests and clippy**

Run: `cargo test && cargo clippy -- -W clippy::all`

**Step 3: Commit**

```bash
git add -A
git commit -m "Update help text and docs for hutt server"
```
