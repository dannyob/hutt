# Multi-Account Support

## Status: Design (not yet implemented)

## Context

Hutt currently supports a single account (takes `accounts[0]`). The config
already has `[[accounts]]` as an array, but nothing uses more than the first
entry. Danny wants to switch between accounts, each with its own maildir,
mu database, SMTP config, and sync command.

## Research: Himalaya's Approach

Himalaya (~/Public/src/himalaya) uses named account tables:

```toml
[accounts.personal]
default = true
email = "danny@example.com"
folder.alias.inbox = "INBOX"
folder.alias.sent = "Sent Mail"

[accounts.personal.backend]
type = "maildir"
root-dir = "~/Maildir/personal"

[accounts.personal.message.send.backend]
type = "smtp"
host = "smtp.example.com"
```

Key ideas borrowed:
- Named accounts (not just an array index)
- `default = true` flag for startup account
- Per-account folder aliases
- Per-account backends

## Design Decisions

### Separate mu databases per account

Each account gets its own mu database via `--muhome`. This avoids
cross-account docid collisions and keeps searches scoped to one account.

A shared database might be possible later (mu can index multiple maildirs)
but separate is simpler and avoids threading confusion between accounts.

### Big reset on account switch

Switching accounts restarts the mu server process with the new `--muhome`.
This is a clean approach: no stale state, no cross-account confusion.
The switch will:

1. Quit the current mu server
2. Start a new mu server with `--muhome <account_muhome>`
3. Clear all app state (envelopes, thread view, selection, undo stack)
4. Load the new account's inbox

### Hardwired to mu + mbsync

No backend abstraction for now. Every account uses mu for indexing and
the user configures their own sync command (typically mbsync).

## Config Format

Keep the existing `[[accounts]]` array but add new fields:

```toml
[[accounts]]
name     = "personal"
default  = true
email    = "danny@example.com"
maildir  = "~/Maildir/personal"
muhome   = "~/.cache/mu/personal"    # --muhome path for mu server

sync_command = "mbsync personal"     # per-account sync (overrides global)

[accounts.smtp]
host             = "smtp.example.com"
port             = 465
encryption       = "ssl"
username         = "danny@example.com"
password_command = "pass email/personal"

[accounts.folders]
inbox   = "/INBOX"
archive = "/All Mail"
drafts  = "/Drafts"
sent    = "/Sent Mail"
trash   = "/Bin"
spam    = "/Junk"

[[accounts]]
name     = "work"
email    = "danny@work.com"
maildir  = "~/Maildir/work"
muhome   = "~/.cache/mu/work"

sync_command = "mbsync work"

[accounts.smtp]
host             = "smtp.work.com"
port             = 587
encryption       = "starttls"
username         = "danny@work.com"
password_command = "pass email/work"

[accounts.folders]
inbox   = "/Inbox"
archive = "/Archive"
drafts  = "/Drafts"
sent    = "/Sent"
trash   = "/Trash"
spam    = "/Spam"
```

New fields:
- `muhome` (required for multi-account) — path to mu database directory
- `default` (optional, bool) — which account to open at startup
- `sync_command` moves from top-level to per-account (top-level kept as fallback)

## What Changes on Account Switch

| Component        | Action                                         |
|------------------|-------------------------------------------------|
| mu server        | Quit + restart with new `--muhome`              |
| App.current_folder | Reset to account's `folders.inbox`           |
| App.envelopes    | Cleared, inbox reloaded                         |
| App.thread_*     | Cleared                                         |
| App.selected_set | Cleared                                         |
| App.undo_stack   | Cleared (docids are per-database)               |
| App.known_folders| Rebuilt from new account's maildir              |
| App.smart_folders| Shared across accounts (or per-account later)   |
| Top bar          | Shows account name                              |
| SMTP config      | Points to new account's smtp section            |
| Compose From:    | Uses new account's email                        |
| sync_command     | Uses account's sync_command (or global fallback) |

## Implementation Steps

### 1. Config changes

- Add `muhome: Option<String>` and `default: Option<bool>` to `AccountConfig`
- Move `sync_command` to `AccountConfig` (keep top-level as fallback)
- Add helper: `Config::default_account() -> &AccountConfig`
- Add helper: `Config::account_by_name(name) -> Option<&AccountConfig>`

### 2. Fix hardcoded folder paths

Before multi-account, fix the triage actions to use the active account's
folder config instead of hardcoded paths:

- `Action::Archive` -> `account.folders.archive` (not `/Archive`)
- `Action::Trash` -> `account.folders.trash` (not `/Trash`)
- `Action::Spam` -> `account.folders.spam` (not `/Junk`)
- `Action::GoInbox` -> `account.folders.inbox` (not `/Inbox`)
- etc.

Add `App.active_account_index: usize` to track which account is active.

### 3. mu server: support --muhome

- `MuClient::start()` takes an optional `muhome` path
- Passes `--muhome <path>` to the mu server command
- Add `MuClient::restart(muhome)` or just quit + start new

### 4. App state: active account tracking

New fields:
```rust
pub active_account: usize,  // index into config.accounts
```

New methods:
```rust
fn active_account(&self) -> &AccountConfig
fn switch_account(&mut self, index: usize) -> Result<()>
```

`switch_account()`:
1. Quit mu server
2. Start new mu server with account's muhome
3. Clear envelopes, thread state, undo stack, selection
4. Rebuild known_folders
5. Navigate to account's inbox
6. Update status: "Switched to <account_name>"

### 5. UI: account switching

- Keybinding: could use a sequence like `g 1`, `g 2` for account 1, 2
  or add accounts to the command palette
- Top bar: show `[account_name]` indicator
- Folder picker: optionally show account name
- Status bar hint when multiple accounts configured

### 6. Per-account sync

`Action::SyncMail` uses the active account's `sync_command`, falling
back to the global `sync_command` if not set.

### 7. Compose: correct From address

Already uses `accounts.first().email` — change to `active_account().email`.

## Outstanding Questions

- Smart folders: shared across accounts or per-account?
  (Shared is simpler; queries might not make sense across different maildirs)
- Account switching keybinding: `g 1`/`g 2`? Command palette entry?
  Something else?
- Should `muhome` be auto-derived from account name if not specified?
  e.g., `~/.cache/mu/<account_name>`
- Initial mu database setup: should hutt run `mu init --muhome <path>
  --maildir <maildir>` automatically if the database doesn't exist?

## Known Prerequisite Bugs

- Hardcoded folder paths in triage actions (step 2 above)
- IPC compose path discards to/subject from HuttUrl::Compose
