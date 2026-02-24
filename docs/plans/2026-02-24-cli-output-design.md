# Design: `--sexp` and `--json` output for remote commands

**Date:** 2026-02-24

## Goal

Make `hutt remote` commands return structured, scriptable output. Currently
they fire-and-forget over IPC with no response. Add `--sexp` and `--json`
flags so commands emit envelope data that's useful for scripting and
automation.

## Overview

Two changes:

1. **Bidirectional IPC** — as designed in `2026-02-23-hutt-server-design.md`,
   make the IPC socket request/response so the TUI sends results back.
2. **CLI output formatting** — `--sexp`, `--json`, and `--wrapped` flags on
   `hutt remote` that format the response for stdout.

## Part 1: Bidirectional IPC

Current protocol: client connects → writes JSON → shuts down write half →
disconnects (no response).

New protocol: client connects → writes JSON → shuts down write half →
**reads JSON response** → disconnects.

Uses the same `IpcResponse` enum from the hutt server design:

```rust
#[derive(Serialize, Deserialize)]
enum IpcResponse {
    Ok,
    Error { message: String },
    MuFrames(Vec<String>),
}
```

### Server side (TUI)

`IpcListener::accept()` changes to return the `UnixStream` alongside the
parsed command, so the caller can write the response back:

```rust
pub async fn accept(&self) -> Result<(IpcCommand, UnixStream)>
```

`handle_ipc_command` returns `IpcResponse` instead of `Result<()>`. After
handling, the event loop serializes the response as JSON and writes it back
on the stream.

### Client side (CLI)

`send_ipc_command` becomes `send_ipc_command_and_recv`:

```rust
pub async fn send_ipc_command(cmd: &IpcCommand) -> Result<IpcResponse>
```

It writes the command JSON, shuts down the write half, then reads the full
response JSON from the stream before disconnecting.

## Part 2: Capturing raw sexp strings

The TUI needs to return raw mu sexp strings, not re-serialized Rust structs.
This avoids lossy round-trips and is compatible with the `MuCommand` proxying
that `hutt server` will need.

Add a capturing variant of `find` to `MuClient`:

```rust
pub async fn find_capturing(&mut self, query: &str, opts: &FindOpts)
    -> Result<(Vec<Envelope>, Vec<String>)>
```

This works like `find()` but also saves each envelope's raw sexp string in
the returned `Vec<String>`. The normal `find()` is unchanged.

`handle_ipc_command` calls `find_capturing` when processing `Open` (message),
`Thread`, `Search`, and `Navigate` commands, and returns
`IpcResponse::MuFrames(sexp_strings)`.

Commands that don't load envelopes (`Compose`, `Quit`) return `IpcResponse::Ok`.
Errors return `IpcResponse::Error { message }`.

## Part 3: What each command returns

| Command | MuFrames contains |
|---------|-------------------|
| `open <msgid>` | The single envelope matching the message-id |
| `thread <msgid>` | All envelopes in the thread |
| `search <query>` | All matching envelopes (up to max_num) |
| `navigate <folder>` | All envelopes in the folder (up to max_num) |
| `compose` | Nothing (returns `Ok`) |
| `quit` | Nothing (returns `Ok`) |

## Part 4: CLI flags and output formatting

```
hutt remote [--sexp|--json] [--wrapped] <COMMAND> [ARGS]
```

`--sexp` and `--json` are mutually exclusive. Without either, output is
silent (backwards compatible — the command still executes in the TUI).
`--wrapped` is only meaningful with `--sexp` or `--json`.

### `--sexp` output

**Unwrapped (default)** — one envelope plist per line, straight from mu:

```
(:docid 42 :subject "Hello" :path "/mail/Inbox/cur/123:2,S" :from ((:email "alice@b.com" :name "Alice")) :date (27028 6999 0) :flags (seen) :maildir "/Inbox")
(:docid 43 :subject "Re: Hello" :path "/mail/Inbox/cur/124:2,S" ...)
```

**Wrapped** (`--wrapped`) — mu-compatible response:

```
(:headers ((:docid 42 ...) (:docid 43 ...)) :found 2)
```

### `--json` output

**Unwrapped (default)** — ndjson, one JSON object per line:

```json
{"docid":42,"subject":"Hello","path":"/mail/Inbox/cur/123:2,S","from":[{"email":"alice@b.com","name":"Alice"}],"date":"2026-02-24T10:30:00Z","flags":["seen"],"maildir":"/Inbox"}
```

**Wrapped** (`--wrapped`):

```json
{"headers":[...],"found":2}
```

### Error output

Errors go to stdout in structured format and exit non-zero:

```
# --sexp
(:error "message not found: abc@example.com")

# --json
{"error":"message not found: abc@example.com"}
```

Without `--sexp`/`--json`, errors print to stderr as today.

### Commands returning no data

`compose` and `quit` emit nothing in unwrapped mode. In wrapped mode:
`(:found 0)` for sexp, `{"found":0}` for JSON.

## Part 5: Sexp-to-JSON conversion

Runs client-side in the `hutt remote` process after receiving `MuFrames`.
Parses each sexp string with `lexpr` and converts to `serde_json::Value`.

New function in `mu_sexp.rs`:

```rust
pub fn sexp_to_json(sexp: &str) -> Result<serde_json::Value>
```

Conversion rules:

| Sexp | JSON |
|------|------|
| `:keyword value` pairs (plist) | `{"keyword": value}` |
| `"string"` | `"string"` |
| `42` (number) | `42` |
| `t` / `nil` symbols | `true` / `false` |
| `(sym1 sym2)` (symbol lists, e.g. flags) | `["sym1", "sym2"]` |
| `((:k v ...) (:k v ...))` (list of plists) | `[{...}, {...}]` |
| `(high low micro)` at `:date` key | `"2026-02-24T10:30:00Z"` (ISO 8601) |

The `:date` key is the only special case — detected by key name, the Emacs
time triple `(high low micro)` is converted to ISO 8601. Everything else is
a generic recursive sexp→JSON walk.

## Compatibility with hutt server design

This design shares the same `IpcResponse` enum and bidirectional IPC protocol
as the hutt server design (`2026-02-23-hutt-server-design.md`). The
`MuFrames(Vec<String>)` variant is used by both:

- **Remote commands**: TUI runs the mu query, captures raw sexp strings,
  returns them as `MuFrames`.
- **hutt server** (future): TUI proxies a raw sexp command to mu, captures
  response frames, returns them as `MuFrames`.

The sexp-to-JSON conversion in `mu_sexp.rs` will also be useful for `hutt
server --eval` if JSON output is ever added there.

## Examples

```bash
# Search and get ndjson
hutt r --json search from:alice subject:project
{"docid":42,"subject":"Project update","path":"/mail/Inbox/cur/123:2,S",...}
{"docid":43,"subject":"Re: Project update","path":"/mail/Inbox/cur/124:2,S",...}

# Get file paths for all messages from alice today
hutt r --json search 'from:alice date:today' | jq -r '.path'
/mail/Inbox/cur/123:2,S
/mail/Inbox/cur/124:2,S

# Open a thread and get mu-compatible sexp
hutt r --sexp --wrapped thread abc@example.com
(:headers ((:docid 42 ...) (:docid 43 ...) (:docid 44 ...)) :found 3)

# Errors are structured too
hutt r --json open nonexistent@example.com
{"error":"message not found: nonexistent@example.com"}
# exit code 1
```

## What's NOT in scope

- `--sexp`/`--json` on non-remote commands (e.g. `hutt config`) — later
- `MuCommand` proxying — that's the hutt server feature
- Message body content in the output — envelopes only (use `:path` to read
  the raw message file)
