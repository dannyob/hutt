# Bidirectional IPC, CLI Output, and hutt server Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make IPC bidirectional (request/response), add `--sexp`/`--json` output to `hutt remote`, and implement `hutt server` as a drop-in mu server proxy. Combines designs from `2026-02-24-cli-output-design.md` and `2026-02-23-hutt-server-design.md`.

**Architecture:** Three phases — (1) bidirectional IPC in `links.rs` with `IpcResponse` type, (2) `--sexp`/`--json`/`--wrapped` output formatting for `hutt remote` commands with `find_capturing` on `MuClient` and `sexp_to_json` conversion, (3) `hutt server` CLI shim that proxies raw S-expressions through hutt's IPC using `MuCommand`.

**Tech Stack:** Rust, tokio, lexpr, serde_json, Unix domain sockets, mu S-expression wire protocol (0xfe/0xff framing)

**Design docs:**
- `docs/plans/2026-02-24-cli-output-design.md`
- `docs/plans/2026-02-23-hutt-server-design.md`

**Key design decisions:**
- `find()` delegates to `find_capturing()` internally — no code duplication
- IPC handlers keep calling `load_folder()` / `navigate_folder()` / `open_thread()` for correct TUI state, then do a second `find_capturing()` call for the IPC response data (IPC commands are infrequent CLI invocations, so the double query is fine)
- `UnixStream` is passed through the mpsc channel directly — no oneshot channels or extra spawned tasks
- `handle_ipc_command` returns `Result<IpcResponse>`; the two event loop call sites convert `Err` to `IpcResponse::Error` inline

---

## Phase 1: Bidirectional IPC

### Task 1: Add `IpcResponse` enum and `encode_frame` helper

**Files:**
- Modify: `src/links.rs` (add `IpcResponse` enum near `IpcCommand`)
- Modify: `src/mu_sexp.rs` (add `encode_frame` function after `read_frame`)

**Step 1: Add `encode_frame` to `src/mu_sexp.rs`**

Add after the `read_frame` function (around line 60):

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

Add test in the existing `#[cfg(test)] mod tests` block:

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

**Step 2: Run test**

Run: `cargo test test_encode_frame_roundtrip`
Expected: PASS

**Step 3: Add `IpcResponse` enum to `src/links.rs`**

Add after the `IpcCommand` enum (around line 240):

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

**Step 4: Add serialization test**

Add in the existing `#[cfg(test)] mod tests` block in `src/links.rs`:

```rust
#[test]
fn ipc_response_json_roundtrip() {
    let resps = vec![
        IpcResponse::Ok,
        IpcResponse::Error { message: "not found".to_string() },
        IpcResponse::MuFrames {
            frames: vec![
                "(:docid 42 :subject \"Hello\")".to_string(),
                "(:docid 43 :subject \"World\")".to_string(),
            ],
        },
    ];
    for resp in &resps {
        let json = serde_json::to_string(resp).unwrap();
        let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2);
    }
}
```

**Step 5: Run tests**

Run: `cargo test ipc_response_json_roundtrip`
Expected: PASS

**Step 6: Commit**

```bash
git add src/links.rs src/mu_sexp.rs
git commit -m "Add IpcResponse enum and encode_frame helper"
```

---

### Task 2: Make IPC bidirectional (links.rs)

**Files:**
- Modify: `src/links.rs` (change `accept` to return stream, add `send_response`, update `send_ipc_command` to read response, make `socket_path` public)

**Step 1: Change `IpcListener::accept` to return stream alongside command**

Replace the `accept` method (around line 363):

```rust
/// Accept a single connection, read a JSON-encoded `IpcCommand`,
/// and return it along with the stream for sending a response.
pub async fn accept(&self) -> Result<(IpcCommand, UnixStream)> {
    let (mut stream, _addr) = self
        .listener
        .accept()
        .await
        .context("accepting IPC connection")?;

    // Read until the client shuts down their write side
    let mut buf = Vec::with_capacity(4096);
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

**Step 2: Add `send_response` helper**

Add after the `Drop` impl for `IpcListener`:

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

Replace the function (around line 389):

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

**Step 4: Make `socket_path` public**

Change `fn socket_path()` to `pub fn socket_path()` (around line 330).

**Step 5: Run `cargo check`**

Expected: compile errors in `src/tui/mod.rs` and `src/main.rs` because signatures changed. Fixed in the next tasks.

**Step 6: Commit**

```bash
git add src/links.rs
git commit -m "Make IPC bidirectional: accept returns stream, send reads response

Compile errors in tui/mod.rs and main.rs expected — fixed next."
```

---

### Task 3: Update TUI event loop for bidirectional IPC

**Files:**
- Modify: `src/tui/mod.rs` (IPC listener task, channel type, handle_ipc_command return type, all call sites)

**Step 1: Update imports**

In `src/tui/mod.rs`, change the links import (line 33):

```rust
use crate::links::{self, HuttUrl, IpcCommand, IpcListener, IpcResponse};
```

**Step 2: Change `handle_ipc_command` return type**

Change signature (around line 1776) from:
```rust
async fn handle_ipc_command(&mut self, cmd: IpcCommand) -> Result<()> {
```
to:
```rust
async fn handle_ipc_command(&mut self, cmd: IpcCommand) -> Result<IpcResponse> {
```

Change every match arm's ending from bare (no return) to `Ok(IpcResponse::Ok)`:

- `HuttUrl::Message` arm: add `Ok(IpcResponse::Ok)` at the end (after `self.set_status(...)`)
- `HuttUrl::Thread` arm: add `Ok(IpcResponse::Ok)` at the end (both the found and not-found branches)
- `HuttUrl::Search` arm: add `Ok(IpcResponse::Ok)` at the end
- `HuttUrl::Compose` arm: add `Ok(IpcResponse::Ok)` at the end
- `IpcCommand::Navigate` arm: add `Ok(IpcResponse::Ok)` at the end
- `IpcCommand::Quit` arm: change to `self.should_quit = true; Ok(IpcResponse::Ok)`

Remove the trailing `Ok(())` at the end of the function (each arm now returns).

**Step 3: Change IPC channel to carry `(IpcCommand, UnixStream)`**

Around line 2668, change:
```rust
let (ipc_tx, mut ipc_rx) = tokio::sync::mpsc::unbounded_channel::<IpcCommand>();
```
to:
```rust
let (ipc_tx, mut ipc_rx) = tokio::sync::mpsc::unbounded_channel::<(IpcCommand, tokio::net::UnixStream)>();
```

**Step 4: Update IPC listener task**

Replace the listener spawn block (around line 2670-2692). The task accepts the connection, reads the command, sends `(cmd, stream)` through the channel. The event loop writes the response after handling:

```rust
Some(tokio::spawn(async move {
    debug_log!("IPC listener started");
    loop {
        match listener.accept().await {
            Ok((cmd, stream)) => {
                debug_log!("IPC accepted: {:?}", cmd);
                if tx.send((cmd, stream)).is_err() {
                    debug_log!("IPC channel closed, exiting");
                    break;
                }
            }
            Err(e) => {
                debug_log!("IPC accept error: {}", e);
                continue;
            }
        }
    }
}))
```

**Step 5: Update drain loop**

Around line 3189, change:
```rust
while let Ok(cmd) = ipc_rx.try_recv() {
    debug_log!("IPC drain: {:?}", cmd);
    if let Err(e) = app.handle_ipc_command(cmd).await {
        app.set_status(format!("IPC error: {}", e));
    }
}
```
to:
```rust
while let Ok((cmd, mut stream)) = ipc_rx.try_recv() {
    debug_log!("IPC drain: {:?}", cmd);
    let resp = match app.handle_ipc_command(cmd).await {
        Ok(resp) => resp,
        Err(e) => {
            app.set_status(format!("IPC error: {}", e));
            IpcResponse::Error { message: e.to_string() }
        }
    };
    if let Err(e) = links::send_response(&mut stream, &resp).await {
        debug_log!("IPC response error: {}", e);
    }
}
```

**Step 6: Update select loop**

Around line 3240, change:
```rust
cmd = ipc_rx.recv() => {
    if let Some(cmd) = cmd {
        debug_log!("IPC select: {:?}", cmd);
        if let Err(e) = app.handle_ipc_command(cmd).await {
            app.set_status(format!("IPC error: {}", e));
        }
    }
    continue;
}
```
to:
```rust
cmd = ipc_rx.recv() => {
    if let Some((cmd, mut stream)) = cmd {
        debug_log!("IPC select: {:?}", cmd);
        let resp = match app.handle_ipc_command(cmd).await {
            Ok(resp) => resp,
            Err(e) => {
                app.set_status(format!("IPC error: {}", e));
                IpcResponse::Error { message: e.to_string() }
            }
        };
        if let Err(e) = links::send_response(&mut stream, &resp).await {
            debug_log!("IPC response error: {}", e);
        }
    }
    continue;
}
```

**Step 7: Run `cargo check`**

Expected: should compile (except main.rs — next task).

**Step 8: Commit**

```bash
git add src/tui/mod.rs
git commit -m "Update TUI event loop for bidirectional IPC

handle_ipc_command returns Result<IpcResponse>. UnixStream passed
through channel. Event loop writes response directly to stream."
```

---

### Task 4: Update `run_remote` in main.rs

**Files:**
- Modify: `src/main.rs`

**Step 1: Update `run_remote` to handle `IpcResponse`**

At the end of `run_remote` (around line 182), change:
```rust
links::send_ipc_command(&cmd).await
```
to:
```rust
let resp = links::send_ipc_command(&cmd).await?;
match resp {
    links::IpcResponse::Ok | links::IpcResponse::MuFrames { .. } => Ok(()),
    links::IpcResponse::Error { message } => {
        bail!("hutt: {}", message);
    }
}
```

**Step 2: Run `cargo check` and `cargo test`**

Run: `cargo check && cargo test`
Expected: all pass, everything compiles.

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "Update run_remote to handle IpcResponse"
```

---

## Phase 2: CLI Output (`--sexp`, `--json`)

### Task 5: Add `find_capturing` and make `find` delegate to it

**Files:**
- Modify: `src/mu_client.rs` (add `find_capturing`, refactor `find` to use it)

**Step 1: Rename existing `find` to `find_capturing`**

Rename the existing `find` method (around line 233) to `find_capturing`. Change its return type from `Result<Vec<Envelope>>` to `Result<(Vec<Envelope>, Vec<String>)>`. In the body, add a `raw_sexps` vec and capture individual envelope sexp strings:

```rust
/// Run a find query and collect envelopes plus individual raw sexp strings.
/// Each string in the returned Vec is one envelope's sexp plist (re-serialized
/// from the parsed Value — semantically identical to mu's output).
pub async fn find_capturing(
    &mut self,
    query: &str,
    opts: &FindOpts,
) -> Result<(Vec<Envelope>, Vec<String>)> {
    let mut cmd = format!(
        "(find :query \"{}\" :sortfield :{} :maxnum {}",
        escape_string(query),
        opts.sort_field,
        opts.max_num,
    );
    if opts.threads {
        cmd.push_str(" :threads t");
    }
    if opts.descending {
        cmd.push_str(" :descending t");
    }
    if opts.include_related {
        cmd.push_str(" :include-related t");
    }
    cmd.push(')');

    self.send(&cmd).await?;

    let mut envelopes = Vec::new();
    let mut raw_sexps = Vec::new();
    loop {
        let value = self.reader.next_frame().await?;
        if mu_sexp::is_erase(&value) {
            continue;
        }
        if let Some(err) = mu_sexp::is_error(&value) {
            bail!("mu find error: {}", err);
        }
        if mu_sexp::is_found(&value).is_some() {
            break;
        }
        // Extract individual envelopes from :headers list
        if let Some(headers) = mu_sexp::plist_get(&value, "headers") {
            if let Some(cons) = headers.as_cons() {
                for pair in cons.iter() {
                    let env_value = pair.car();
                    raw_sexps.push(env_value.to_string());
                    match mu_sexp::parse_envelope(env_value) {
                        Ok(env) => envelopes.push(env),
                        Err(e) => mu_log!("find_capturing: skip envelope: {}", e),
                    }
                }
            }
        }
    }
    Ok((envelopes, raw_sexps))
}
```

**Step 2: Add `find` as a thin wrapper**

```rust
/// Run a find query and collect all envelope results.
pub async fn find(&mut self, query: &str, opts: &FindOpts) -> Result<Vec<Envelope>> {
    let (envelopes, _) = self.find_capturing(query, opts).await?;
    Ok(envelopes)
}
```

**Step 3: Run `cargo test`**

All existing tests should pass unchanged — `find` has the same signature and behavior.

**Step 4: Commit**

```bash
git add src/mu_client.rs
git commit -m "Add find_capturing, refactor find to delegate to it

find_capturing returns (Vec<Envelope>, Vec<String>) where the strings
are individual envelope sexp plists. find() is now a thin wrapper."
```

---

### Task 6: Return MuFrames from IPC envelope commands

**Files:**
- Modify: `src/tui/mod.rs` (update `handle_ipc_command` to capture and return sexp data)

The principle: each IPC handler keeps doing what it does today (load_folder, navigate_folder, open_thread) for correct TUI state, then does a second `find_capturing` call to get the raw sexp strings for the response.

**Step 1: Add a helper method for IPC envelope capture**

Add to `impl App`:

```rust
/// Run find_capturing to get raw sexp strings for IPC response.
/// Separate from the TUI's own find/load_folder so TUI state is unaffected.
async fn capture_envelopes(&mut self, query: &str, opts: &FindOpts) -> IpcResponse {
    match self.mu.find_capturing(query, opts).await {
        Ok((_envelopes, raw_sexps)) => IpcResponse::MuFrames { frames: raw_sexps },
        Err(e) => IpcResponse::Error { message: format!("capture error: {}", e) },
    }
}
```

**Step 2: Update `HuttUrl::Message` arm**

The current code sets `self.current_folder = query` then calls `self.load_folder()`. Keep that. After it, capture for the response:

```rust
HuttUrl::Message { id, account } => {
    self.switch_to_account_if_needed(&account).await?;
    let query = format!("msgid:{}", id);
    debug_log!("IPC Message: query={}", query);
    self.mode = InputMode::Normal;
    self.thread_messages.clear();
    self.current_folder = query.clone();
    match self.load_folder().await {
        Ok(()) => debug_log!("IPC Message: loaded {} envelopes", self.envelopes.len()),
        Err(e) => debug_log!("IPC Message: load error: {}", e),
    }
    self.set_status(format!("Opened message {}", id));
    Ok(self.capture_envelopes(&query, &FindOpts::default()).await)
}
```

**Step 3: Update `HuttUrl::Thread` arm**

Keep existing TUI logic (find → open_thread). Capture the thread with `include_related`:

```rust
HuttUrl::Thread { id, account } => {
    self.switch_to_account_if_needed(&account).await?;
    let query = format!("msgid:{}", id);
    debug_log!("IPC Thread: query={}", query);
    self.mode = InputMode::Normal;
    self.thread_messages.clear();
    let result = self
        .mu
        .find(&query, &crate::mu_client::FindOpts::default())
        .await;
    debug_log!("IPC Thread: find result: {:?}", result.as_ref().map(|v| v.len()));
    let envelopes = result.unwrap_or_default();
    if let Some(envelope) = envelopes.into_iter().next() {
        self.envelopes = vec![envelope];
        self.selected = 0;
        match self.open_thread().await {
            Ok(()) => debug_log!("IPC Thread: opened, {} messages", self.thread_messages.len()),
            Err(e) => debug_log!("IPC Thread: open_thread error: {}", e),
        }
        self.set_status(format!("Opened thread {}", id));
        // Capture thread envelopes for response
        let opts = FindOpts {
            include_related: true,
            descending: false,
            ..FindOpts::default()
        };
        Ok(self.capture_envelopes(&query, &opts).await)
    } else {
        debug_log!("IPC Thread: message not found");
        self.set_status(format!("Message not found: {}", id));
        Ok(IpcResponse::Error {
            message: format!("message not found: {}", id),
        })
    }
}
```

**Step 4: Update `HuttUrl::Search` arm**

```rust
HuttUrl::Search { query, account } => {
    self.switch_to_account_if_needed(&account).await?;
    debug_log!("IPC Search: query={}", query);
    self.mode = InputMode::Normal;
    self.thread_messages.clear();
    self.current_folder = query.clone();
    match self.load_folder().await {
        Ok(()) => debug_log!("IPC Search: loaded {} envelopes", self.envelopes.len()),
        Err(e) => debug_log!("IPC Search: load error: {}", e),
    }
    self.set_status(format!("Search: {}", query));
    // Capture uses build_query() since load_folder may expand the query
    let expanded = self.build_query();
    Ok(self.capture_envelopes(&expanded, &FindOpts::default()).await)
}
```

**Step 5: Update `IpcCommand::Navigate` arm**

```rust
IpcCommand::Navigate { folder, account } => {
    self.switch_to_account_if_needed(&account).await?;
    debug_log!("IPC Navigate: folder={}", folder);
    self.mode = InputMode::Normal;
    self.thread_messages.clear();
    match self.navigate_folder(&folder).await {
        Ok(()) => debug_log!("IPC Navigate: loaded {} envelopes", self.envelopes.len()),
        Err(e) => debug_log!("IPC Navigate: error: {}", e),
    }
    // Capture using the expanded query (after navigate set current_folder)
    let expanded = self.build_query();
    Ok(self.capture_envelopes(&expanded, &FindOpts::default()).await)
}
```

**Step 6: `Compose` and `Quit` arms stay as `Ok(IpcResponse::Ok)`**

No changes needed — they don't return envelope data.

**Step 7: Run `cargo check`**

**Step 8: Commit**

```bash
git add src/tui/mod.rs
git commit -m "Return MuFrames from IPC envelope commands

Each handler keeps its existing TUI state management (load_folder,
navigate_folder, open_thread) then does a second find_capturing
for the IPC response."
```

---

### Task 7: Add `sexp_to_json` conversion

**Files:**
- Modify: `src/mu_sexp.rs` (add `sexp_to_json` and helpers)

**Step 1: Add the conversion functions**

Add at the end of `src/mu_sexp.rs` (before `#[cfg(test)]`):

```rust
/// Convert a mu sexp plist string to a JSON value.
///
/// Special handling:
/// - `:date` / `:changed` keys: Emacs time triple → ISO 8601 string
/// - Plist keyword-value pairs → JSON object
/// - Symbol lists (flags) → JSON string arrays
/// - `t` / `nil` → `true` / `false`
/// - Nested plists and lists of plists → nested objects/arrays
pub fn sexp_to_json(sexp: &str) -> Result<serde_json::Value> {
    let value = parse_sexp(sexp)?;
    Ok(value_to_json(&value, None))
}

/// Recursive conversion of a lexpr Value to serde_json::Value.
/// `parent_key` is used for context-sensitive conversion (e.g. :date).
fn value_to_json(value: &Value, parent_key: Option<&str>) -> serde_json::Value {
    // Nil
    if value.is_nil() {
        return serde_json::Value::Null;
    }

    // String
    if let Some(s) = value.as_str() {
        return serde_json::Value::String(s.to_string());
    }

    // Integer
    if let Some(n) = value.as_i64() {
        return serde_json::json!(n);
    }

    // Float
    if let Some(n) = value.as_f64() {
        return serde_json::json!(n);
    }

    // Symbol: t → true, nil → false, others → string
    if let Some(sym) = value.as_symbol() {
        return match sym {
            "t" => serde_json::Value::Bool(true),
            "nil" => serde_json::Value::Bool(false),
            _ => serde_json::Value::String(sym.to_string()),
        };
    }

    // Bare keyword
    if let Some(kw) = value.as_keyword() {
        return serde_json::Value::String(format!(":{}", kw));
    }

    // Cons cell (list)
    if let Some(cons) = value.as_cons() {
        let items: Vec<&Value> = cons.iter().map(|pair| pair.car()).collect();
        if items.is_empty() {
            return serde_json::Value::Array(vec![]);
        }

        // Plist: first element is a keyword
        if items[0].is_keyword() {
            return plist_to_json(&items);
        }

        // Emacs time triple under :date or :changed key
        if matches!(parent_key, Some("date") | Some("changed") | Some("data-tstamp")) {
            if let Some(dt) = parse_emacs_time(value) {
                return serde_json::Value::String(dt.to_rfc3339());
            }
        }

        // List of plists: first element is itself a cons starting with keyword
        if items[0].as_cons().is_some() {
            let first_items: Vec<&Value> = items[0]
                .as_cons()
                .unwrap()
                .iter()
                .map(|p| p.car())
                .collect();
            if !first_items.is_empty() && first_items[0].is_keyword() {
                return serde_json::Value::Array(
                    items
                        .iter()
                        .map(|item| value_to_json(item, None))
                        .collect(),
                );
            }
        }

        // Plain list of values (symbols, numbers, etc.)
        serde_json::Value::Array(
            items
                .iter()
                .map(|item| value_to_json(item, None))
                .collect(),
        )
    } else {
        // Fallback
        serde_json::Value::String(value.to_string())
    }
}

/// Convert a plist (keyword-value pairs) to a JSON object.
fn plist_to_json(items: &[&Value]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut i = 0;
    while i < items.len() {
        if let Some(key) = items[i].as_keyword() {
            if i + 1 < items.len() {
                let val = value_to_json(items[i + 1], Some(key));
                map.insert(key.to_string(), val);
                i += 2;
            } else {
                map.insert(key.to_string(), serde_json::Value::Bool(true));
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    serde_json::Value::Object(map)
}
```

**Step 2: Add tests**

Add in the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_sexp_to_json_envelope() {
    let sexp = r#"(:docid 42 :subject "Hello World" :from ((:email "alice@example.com" :name "Alice")) :to ((:email "bob@example.com")) :date (27028 6999 0) :flags (seen list) :maildir "/Inbox" :path "/mail/Inbox/cur/123:2,S" :message-id "abc@example.com")"#;
    let json = sexp_to_json(sexp).unwrap();

    assert_eq!(json["docid"], 42);
    assert_eq!(json["subject"], "Hello World");
    assert_eq!(json["from"][0]["email"], "alice@example.com");
    assert_eq!(json["from"][0]["name"], "Alice");
    assert_eq!(json["to"][0]["email"], "bob@example.com");
    assert_eq!(json["flags"][0], "seen");
    assert_eq!(json["flags"][1], "list");
    assert_eq!(json["maildir"], "/Inbox");
    assert_eq!(json["path"], "/mail/Inbox/cur/123:2,S");
    assert_eq!(json["message-id"], "abc@example.com");
    // Date should be ISO 8601
    let date_str = json["date"].as_str().unwrap();
    assert!(date_str.contains("2026-"), "expected ISO 8601 date, got: {}", date_str);
}

#[test]
fn test_sexp_to_json_symbols() {
    let sexp = "(:root t :draft nil)";
    let json = sexp_to_json(sexp).unwrap();
    assert_eq!(json["root"], true);
    assert_eq!(json["draft"], false);
}

#[test]
fn test_sexp_to_json_nested_meta() {
    let sexp = r#"(:docid 1 :meta (:level 0 :root t :thread-subject t))"#;
    let json = sexp_to_json(sexp).unwrap();
    assert_eq!(json["meta"]["level"], 0);
    assert_eq!(json["meta"]["root"], true);
}

#[test]
fn test_sexp_to_json_empty_list() {
    let sexp = "(:docid 1 :flags ())";
    let json = sexp_to_json(sexp).unwrap();
    // nil in lexpr for empty list
    assert!(json["flags"].is_null() || json["flags"].is_array());
}
```

**Step 3: Run tests**

Run: `cargo test test_sexp_to_json`
Expected: all PASS

**Step 4: Commit**

```bash
git add src/mu_sexp.rs
git commit -m "Add sexp_to_json: convert mu S-expressions to JSON

Generic recursive walk with special-case :date/:changed → ISO 8601.
Handles plists, nested plists, symbol lists, address lists."
```

---

### Task 8: Add output format flags and formatting logic to `run_remote`

**Files:**
- Modify: `src/main.rs` (parse `--sexp`/`--json`/`--wrapped`, format and print output)

**Step 1: Define output format enum**

Add near the top of `src/main.rs` (after `const VERSION`):

```rust
/// Output format for remote commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Silent,
    Sexp,
    Json,
}
```

**Step 2: Add `extract_output_flags` helper**

```rust
/// Extract --sexp, --json, --wrapped from args, returning (format, wrapped, remaining).
fn extract_output_flags(args: &[String]) -> Result<(OutputFormat, bool, Vec<String>)> {
    let mut format = OutputFormat::Silent;
    let mut wrapped = false;
    let mut rest = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--sexp" => {
                if format == OutputFormat::Json {
                    bail!("--sexp and --json are mutually exclusive");
                }
                format = OutputFormat::Sexp;
            }
            "--json" => {
                if format == OutputFormat::Sexp {
                    bail!("--sexp and --json are mutually exclusive");
                }
                format = OutputFormat::Json;
            }
            "--wrapped" => wrapped = true,
            _ => rest.push(arg.clone()),
        }
    }

    Ok((format, wrapped, rest))
}
```

**Step 3: Add `print_ipc_output` function**

```rust
/// Format and print IPC response according to output flags.
fn print_ipc_output(resp: &links::IpcResponse, format: OutputFormat, wrapped: bool) {
    match resp {
        links::IpcResponse::Ok => {
            if wrapped {
                match format {
                    OutputFormat::Sexp => println!("(:found 0)"),
                    OutputFormat::Json => println!("{{\"found\":0}}"),
                    OutputFormat::Silent => {}
                }
            }
        }
        links::IpcResponse::Error { message } => {
            match format {
                OutputFormat::Sexp => {
                    println!(
                        "(:error \"{}\")",
                        message.replace('\\', "\\\\").replace('"', "\\\"")
                    );
                }
                OutputFormat::Json => {
                    let obj = serde_json::json!({"error": message});
                    println!("{}", obj);
                }
                OutputFormat::Silent => {} // handled via bail! in caller
            }
        }
        links::IpcResponse::MuFrames { frames } => {
            match format {
                OutputFormat::Silent => {}
                OutputFormat::Sexp => {
                    if wrapped {
                        let joined = frames.join(" ");
                        println!("(:headers ({}) :found {})", joined, frames.len());
                    } else {
                        for frame in frames {
                            println!("{}", frame);
                        }
                    }
                }
                OutputFormat::Json => {
                    if wrapped {
                        let json_vals: Vec<serde_json::Value> = frames
                            .iter()
                            .filter_map(|s| mu_sexp::sexp_to_json(s).ok())
                            .collect();
                        let obj = serde_json::json!({
                            "headers": json_vals,
                            "found": frames.len(),
                        });
                        println!("{}", obj);
                    } else {
                        for frame in frames {
                            if let Ok(json) = mu_sexp::sexp_to_json(frame) {
                                println!("{}", json);
                            }
                        }
                    }
                }
            }
        }
    }
}
```

**Step 4: Wire into `run_remote`**

Replace the beginning and end of `run_remote`:

```rust
async fn run_remote(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_remote_help();
        std::process::exit(1);
    }

    let (format, wrapped, args) = extract_output_flags(args)?;

    if args.is_empty() {
        print_remote_help();
        std::process::exit(1);
    }

    let cmd = match args[0].as_str() {
        // ... (all existing command parsing, but using `args` not the original)
```

At the end, replace the `send_ipc_command` + match:

```rust
    let resp = links::send_ipc_command(&cmd).await?;

    // Print structured output if requested
    print_ipc_output(&resp, format, wrapped);

    match &resp {
        links::IpcResponse::Error { message } => {
            if format == OutputFormat::Silent {
                bail!("hutt: {}", message);
            }
            // Already printed structured error above
            std::process::exit(1);
        }
        _ => Ok(()),
    }
}
```

**Step 5: Update `print_remote_help`**

Add before the COMMANDS section:

```
OUTPUT FLAGS:
    --sexp                  Print results as S-expressions (one per line)
    --json                  Print results as JSON (ndjson, one per line)
    --wrapped               Wrap output in a single object/list
```

**Step 6: Run `cargo check`**

**Step 7: Commit**

```bash
git add src/main.rs
git commit -m "Add --sexp/--json/--wrapped output flags to hutt remote

Formats IPC response for scripting. Silent by default (backwards
compatible). sexp emits raw mu plists, json converts via sexp_to_json."
```

---

### Task 9: Update top-level help and run full verification

**Files:**
- Modify: `src/main.rs` (update `print_help`)

**Step 1: Update `print_help`**

Add to OPTIONS:
```
    --sexp                      (remote) Print results as S-expressions
    --json                      (remote) Print results as JSON (ndjson)
    --wrapped                   (remote) Wrap output as single object
```

Add to EXAMPLES:
```
    hutt r --json search from:alice     Search and output ndjson
    hutt r --sexp thread abc@host.com   Thread envelopes as sexp
    hutt r --json search q | jq '.path' Extract file paths with jq
```

**Step 2: Run full test suite and clippy**

Run: `cargo test && cargo clippy -- -W clippy::all`
Expected: all pass, no warnings

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "Update help text with --sexp/--json examples"
```

---

## Phase 3: hutt server

### Task 10: Add `next_frame_raw` and `send_raw` to MuClient

**Files:**
- Modify: `src/mu_sexp.rs` (add `read_frame_raw`)
- Modify: `src/mu_client.rs` (add `next_frame_raw` to `FrameReader`, add `send_raw` to `MuClient`)

**Step 1: Add `read_frame_raw` to `src/mu_sexp.rs`**

Add after `encode_frame`:

```rust
/// Like `read_frame`, but also returns the raw S-expression string.
/// Returns (parsed Value, raw sexp string, bytes consumed).
pub fn read_frame_raw(buf: &[u8]) -> Result<Option<(Value, String, usize)>> {
    let start = match buf.iter().position(|&b| b == 0xfe) {
        Some(pos) => pos,
        None => return Ok(None),
    };

    let sep = match buf[start + 1..].iter().position(|&b| b == 0xff) {
        Some(pos) => start + 1 + pos,
        None => return Ok(None),
    };

    let hex_str = std::str::from_utf8(&buf[start + 1..sep])
        .context("invalid utf-8 in frame length")?;
    let length =
        usize::from_str_radix(hex_str, 16).context("invalid hex length in frame")?;

    let data_start = sep + 1;
    let data_end = data_start + length;

    if buf.len() < data_end {
        return Ok(None);
    }

    let sexp_bytes = &buf[data_start..data_end];
    let sexp_str =
        std::str::from_utf8(sexp_bytes).context("invalid utf-8 in sexp data")?;
    let raw = sexp_str.to_string();
    let value = parse_sexp(sexp_str)?;

    Ok(Some((value, raw, data_end)))
}
```

Add test:

```rust
#[test]
fn test_read_frame_raw() {
    let sexp = "(:found 3 :query \"test\")";
    let encoded = encode_frame(sexp);
    let (value, raw, consumed) = read_frame_raw(&encoded).unwrap().unwrap();
    assert_eq!(consumed, encoded.len());
    assert_eq!(raw, sexp);
    assert_eq!(is_found(&value), Some(3));
}
```

**Step 2: Add `next_frame_raw` to `FrameReader` in `src/mu_client.rs`**

Add after the `next_frame` method:

```rust
/// Like `next_frame`, but also returns the raw sexp string as received from mu.
async fn next_frame_raw(&mut self) -> Result<(Value, String)> {
    loop {
        if let Some((value, raw, consumed)) = mu_sexp::read_frame_raw(&self.buf)? {
            self.buf.drain(..consumed);
            return Ok((value, raw));
        }

        let mut tmp = [0u8; 8192];
        let n = self.stdout.read(&mut tmp).await?;
        if n == 0 {
            bail!("mu server closed stdout");
        }
        self.buf.extend_from_slice(&tmp[..n]);
    }
}
```

**Step 3: Add `send_raw` to `MuClient`**

Add after `poll_index_frame`:

```rust
/// Send a raw S-expression command and collect all response frames
/// as raw strings until a terminal frame.
/// Skips :erase frames. Used for MuCommand proxying.
pub async fn send_raw(&mut self, sexp: &str) -> Result<Vec<String>> {
    self.send(sexp).await?;
    let mut frames = Vec::new();
    loop {
        let (value, raw) = self.reader.next_frame_raw().await?;
        if mu_sexp::is_erase(&value) {
            continue;
        }
        let is_terminal = mu_sexp::is_found(&value).is_some()
            || mu_sexp::is_pong(&value)
            || mu_sexp::is_error(&value).is_some()
            || mu_sexp::is_update(&value)
            || mu_sexp::plist_get_u32(&value, "remove").is_some()
            || mu_sexp::plist_get(&value, "index").is_some()
            || (mu_sexp::plist_get(&value, "info").is_some()
                && mu_sexp::plist_get(&value, "status")
                    .and_then(|v| v.as_symbol()) == Some("complete"));
        frames.push(raw);
        if is_terminal {
            break;
        }
    }
    Ok(frames)
}
```

**Step 4: Run `cargo test test_read_frame_raw` and `cargo check`**

**Step 5: Commit**

```bash
git add src/mu_sexp.rs src/mu_client.rs
git commit -m "Add raw frame capture: read_frame_raw, next_frame_raw, send_raw

For faithful mu server proxying. send_raw preserves original sexp
strings instead of re-serializing from parsed Values."
```

---

### Task 11: Add `MuCommand` variant and handle it

**Files:**
- Modify: `src/links.rs` (add `MuCommand` to `IpcCommand`)
- Modify: `src/tui/mod.rs` (handle `MuCommand`, add `resolve_mu_target`)

**Step 1: Add `MuCommand` variant**

In `src/links.rs`, add to the `IpcCommand` enum:

```rust
MuCommand {
    sexp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muhome: Option<String>,
},
```

Add test:

```rust
#[test]
fn test_mu_command_serde() {
    let cmd = IpcCommand::MuCommand {
        sexp: "(ping)".to_string(),
        account: Some("work".to_string()),
        muhome: None,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: IpcCommand = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2);
}
```

**Step 2: Add `resolve_mu_target` to `App`**

```rust
/// Resolve account index from optional account name / muhome path.
/// muhome takes precedence. Returns None if specified but not found,
/// Some(active_account) if neither specified.
fn resolve_mu_target(&self, account: Option<&str>, muhome: Option<&str>) -> Option<usize> {
    if let Some(mh) = muhome {
        for (idx, _acct) in self.config.accounts.iter().enumerate() {
            if let Some(effective) = self.config.effective_muhome(idx) {
                if effective == mh {
                    return Some(idx);
                }
            }
        }
        return None;
    }
    if let Some(name) = account {
        return self.config.accounts.iter().position(|a| a.name == *name);
    }
    Some(self.active_account)
}
```

**Step 3: Add `MuCommand` arm to `handle_ipc_command`**

```rust
IpcCommand::MuCommand { sexp, account, muhome } => {
    let target_idx = self.resolve_mu_target(account.as_deref(), muhome.as_deref());
    match target_idx {
        Some(idx) => {
            if idx == self.active_account && self.indexing {
                return Ok(IpcResponse::Error {
                    message: "mu server is busy (indexing)".to_string(),
                });
            }
            let mu = if idx == self.active_account {
                &mut self.mu
            } else {
                match self.background_mu.get_mut(&idx) {
                    Some(mu) => mu,
                    None => return Ok(IpcResponse::Error {
                        message: format!(
                            "no mu server for account '{}'",
                            self.config.accounts.get(idx)
                                .map(|a| a.name.as_str()).unwrap_or("?")
                        ),
                    }),
                }
            };
            match mu.send_raw(&sexp).await {
                Ok(frames) => {
                    if sexp.starts_with("(move")
                        || sexp.starts_with("(remove")
                        || sexp.starts_with("(index")
                    {
                        self.invalidate_folder_cache();
                    }
                    Ok(IpcResponse::MuFrames { frames })
                }
                Err(e) => Ok(IpcResponse::Error {
                    message: format!("mu error: {}", e),
                }),
            }
        }
        None => Ok(IpcResponse::Error {
            message: "no matching account for muhome/account".to_string(),
        }),
    }
}
```

**Step 4: Run `cargo test test_mu_command_serde` and `cargo check`**

**Step 5: Commit**

```bash
git add src/links.rs src/tui/mod.rs
git commit -m "Add MuCommand: forward raw S-expressions to mu server

Routes by muhome or account name. Collects raw response frames.
Invalidates folder cache after mutations."
```

---

### Task 12: Implement `hutt server` CLI subcommand

**Files:**
- Modify: `src/main.rs` (add `server` subcommand, helper functions)

**Step 1: Add `server` to CLI dispatch**

In the main CLI match (around line 180), add before `"-h"`:

```rust
"server" => {
    return run_server(&args[i + 1..]).await;
}
```

**Step 2: Add `print_server_help`**

```rust
fn print_server_help() {
    eprintln!(
        "hutt server — mu server proxy (drop-in replacement for mu server)

USAGE:
    hutt server [OPTIONS]

OPTIONS:
    -h, --help              Show help
    --commands              List available mu commands
    --eval TEXT             Evaluate a single mu server expression
    --allow-temp-file       Accepted for compatibility (ignored)
    --muhome <dir>          Select account by muhome path
    --account <name>        Select account by name
    -a <name>               (same as --account)

When hutt is running, proxies through its mu server. Falls back
to standalone mu server otherwise."
    );
}
```

**Step 3: Add `run_server`**

```rust
async fn run_server(args: &[String]) -> Result<()> {
    let mut muhome: Option<String> = None;
    let mut account: Option<String> = None;
    let mut eval: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_server_help();
                return Ok(());
            }
            "--commands" => {
                println!("commands: compose contacts find index move ping quit remove");
                return Ok(());
            }
            "--eval" => {
                i += 1;
                eval = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--eval requires TEXT"))?
                        .clone(),
                );
            }
            "--allow-temp-file" => {}
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

    let hutt_available = links::socket_path().exists();

    if let Some(ref eval_sexp) = eval {
        if hutt_available {
            return run_server_eval(eval_sexp, account, muhome).await;
        }
        return run_mu_fallback(args).await;
    }

    if hutt_available {
        run_server_interactive(account, muhome).await
    } else {
        run_mu_fallback(args).await
    }
}
```

**Step 4: Add `run_server_eval`**

```rust
async fn run_server_eval(
    sexp: &str,
    account: Option<String>,
    muhome: Option<String>,
) -> Result<()> {
    let cmd = links::IpcCommand::MuCommand {
        sexp: sexp.to_string(),
        account,
        muhome,
    };
    let resp = links::send_ipc_command(&cmd).await?;
    match resp {
        links::IpcResponse::MuFrames { frames } => {
            use std::io::Write;
            for frame in &frames {
                let encoded = mu_sexp::encode_frame(frame);
                std::io::stdout().write_all(&encoded)?;
            }
            std::io::stdout().flush()?;
            Ok(())
        }
        links::IpcResponse::Error { message } => bail!("{}", message),
        links::IpcResponse::Ok => Ok(()),
    }
}
```

**Step 5: Add `run_server_interactive`**

```rust
async fn run_server_interactive(
    account: Option<String>,
    muhome: Option<String>,
) -> Result<()> {
    use std::io::Write;
    use tokio::io::AsyncBufReadExt;

    // Synthetic pong greeting
    let greeting = mu_sexp::encode_frame(
        &format!("(:pong \"hutt\" :props (:version \"{}\"))", VERSION),
    );
    std::io::stdout().write_all(&greeting)?;
    std::io::stdout().flush()?;

    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let sexp = line.trim().to_string();
        if sexp.is_empty() {
            continue;
        }
        if sexp == "(quit)" {
            break;
        }

        let cmd = links::IpcCommand::MuCommand {
            sexp,
            account: account.clone(),
            muhome: muhome.clone(),
        };

        let write_error = |msg: &str| -> Result<()> {
            let err_sexp = format!(
                "(:error 1 :message \"{}\")",
                msg.replace('\\', "\\\\").replace('"', "\\\"")
            );
            let encoded = mu_sexp::encode_frame(&err_sexp);
            std::io::stdout().write_all(&encoded)?;
            std::io::stdout().flush()?;
            Ok(())
        };

        match links::send_ipc_command(&cmd).await {
            Ok(resp) => match resp {
                links::IpcResponse::MuFrames { frames } => {
                    for frame in &frames {
                        let encoded = mu_sexp::encode_frame(frame);
                        std::io::stdout().write_all(&encoded)?;
                    }
                    std::io::stdout().flush()?;
                }
                links::IpcResponse::Error { message } => {
                    write_error(&message)?;
                }
                links::IpcResponse::Ok => {}
            },
            Err(e) => {
                write_error(&e.to_string())?;
            }
        }
    }

    Ok(())
}
```

**Step 6: Add `run_mu_fallback`**

```rust
async fn run_mu_fallback(args: &[String]) -> Result<()> {
    let mut mu_args = vec!["server".to_string()];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--account" | "-a" => { i += 1; } // skip (not a mu flag)
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

**Step 7: Run `cargo check` and `cargo clippy`**

**Step 8: Commit**

```bash
git add src/main.rs
git commit -m "Add 'hutt server' CLI: drop-in mu server replacement

Interactive and --eval modes. Proxies through running hutt via IPC.
Falls back to standalone mu when hutt unavailable."
```

---

### Task 13: Update help text, CLAUDE.md, and final verification

**Files:**
- Modify: `src/main.rs` (update `print_help`)
- Modify: `CLAUDE.md`

**Step 1: Update `print_help`**

Add to USAGE:
```
    hutt server [OPTIONS]            Drop-in mu server replacement (proxies through hutt)
```

Add SERVER section and update EXAMPLES:
```
    hutt server                     Interactive mu server proxy
    hutt server --eval '(ping)'    Single command evaluation
    hutt server --muhome ~/.mu/work Select account by muhome
```

**Step 2: Update CLAUDE.md**

Under "Key subsystems", add:
```
- **hutt server** (`main.rs:run_server`): Drop-in `mu server` replacement.
  Proxies raw S-expressions through hutt's running mu server via bidirectional
  IPC. Falls back to standalone `mu server` when hutt isn't running.
```

Update the URI/IPC description to mention bidirectional:
```
- **URI schemes** (`links.rs`): ... Bidirectional IPC: commands return
  `IpcResponse` (Ok/Error/MuFrames). `--sexp`/`--json`/`--wrapped` flags
  on `hutt remote` format the response for scripting.
```

**Step 3: Run full test suite and clippy**

Run: `cargo test && cargo clippy -- -W clippy::all`
Expected: all pass, no warnings

**Step 4: Commit**

```bash
git add -A
git commit -m "Update help text and CLAUDE.md for bidirectional IPC, CLI output, hutt server"
```
