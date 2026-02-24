# Design: `hutt server` — mu server proxy

**Date:** 2026-02-23

## Goal

Allow external tools to use hutt's running mu server, avoiding the exclusive
Xapian lock problem. `hutt server` is a drop-in replacement for `mu server`
that proxies commands through hutt's IPC socket.

## Overview

Three changes:

1. **Bidirectional IPC** — existing IPC becomes request/response
2. **`MuCommand` IPC command** — forwards raw S-expressions to hutt's mu server
3. **`hutt server` CLI** — thin shim speaking mu wire protocol on stdin/stdout

## Part 1: Bidirectional IPC

Current protocol: client connects → sends JSON → disconnects (no response).

New protocol: client connects → sends JSON → reads JSON response → disconnects.

```rust
#[derive(Serialize, Deserialize)]
enum IpcResponse {
    Ok,
    Error { message: String },
    MuFrames(Vec<String>),
}
```

Existing commands (`Open`, `Navigate`, `Quit`) return `Ok` or `Error`.
`hutt remote` CLI prints errors to stderr and exits non-zero on `Error`.

## Part 2: MuCommand IPC command

```rust
// Added to IpcCommand enum
MuCommand {
    sexp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muhome: Option<String>,
}
```

When hutt receives `MuCommand`:

1. Resolve the target mu server: match `muhome` against account configs first,
   then `account` by name, then fall back to active account. If no match,
   return `Error { message: "no matching account" }` (the shim will fall back
   to standalone mu).
2. If an index operation is in progress, queue the command until it finishes
3. Forward the raw S-expression string to mu via `send()`
4. Read frames until a terminal frame (`:found`, `:update`, `:pong`, `:error`,
   `:contacts`, etc.)
5. Collect frames as strings into `IpcResponse::MuFrames(vec)`
6. Write response JSON back to the IPC client

Background mu servers for non-active accounts are already tracked by hutt,
so routing by account name is just a lookup.

## Part 3: `hutt server` CLI

```
hutt server [OPTIONS]

Options:
  -h, --help              Show help information
  --commands              List available commands
  --eval TEXT             Evaluate mu server expression
  --allow-temp-file       Allow for the temp-file optimization
  --muhome <dir>          Ignored (hutt manages mu homes); accepted for compat
  --account <name>        Route commands to a specific account's mu server
```

`--muhome` and `--account` are alternative ways to pick which mu server to use.
`--muhome` matches against each account's configured muhome path. `--account`
matches by name. If both are given, `--muhome` takes precedence.

### Fallback to standalone mu server

If hutt isn't running, or the `--muhome` doesn't match any account hutt knows
about, `hutt server` falls back to spawning `mu server` directly with the
original command-line arguments. This makes `hutt server` a complete drop-in
replacement for `mu server` — it proxies through hutt when possible, and
runs mu standalone when not.

### Interactive mode (default)

- Emit mu-style greeting (version info, `mu>` prompt)
- Read length-prefixed S-expression frames from stdin
- For each frame: send `MuCommand { sexp, account }` via IPC to running hutt
- Receive `MuFrames(vec)` response
- Write each frame to stdout in mu wire format (length prefix + S-expression)
- Print `mu>` prompt after each command completes
- Exit on EOF or `(quit)` command

### `--eval TEXT` mode

- Send the single S-expression via IPC
- Print response frames to stdout
- Exit

### `--commands` mode

- Print the list of mu server commands (find, move, index, ping, etc.)
- Exit (no IPC needed)

### Error handling

- If hutt isn't running (can't connect to IPC socket): fall back to
  `mu server` with the original command-line args
- If IPC returns `Error` with "no matching account": fall back to
  `mu server` with the original command-line args
- Other `Error` responses: print to stderr, continue (interactive) or exit
  (eval mode)

## Wire format

mu server uses length-prefixed framing:

```
\376<hex-length>\377<sexp-data>
```

`hutt server` reads this from stdin and writes it to stdout. The S-expression
content between the framing markers is what gets forwarded as the `sexp` string
in `MuCommand`. Response frames from `MuFrames` get wrapped back in the same
framing format for stdout.

## Interleaving

mu server is single-command-at-a-time. Hutt's event loop is already serial —
it handles one action at a time. IPC commands arrive as events in the same loop,
so they're naturally serialized with hutt's own mu usage.

The one edge case is in-progress indexing, which polls frames across multiple
event loop ticks. Proxy commands queue behind indexing until it completes.

## What's NOT in scope

- Multiple simultaneous proxy clients (one at a time is fine)
