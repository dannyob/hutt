# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is hutt?

A fast, keyboard-driven TUI email client written in Rust, inspired by Superhuman's UX. Uses **mu** as the mail indexing backend to query Maildir. Vim-style navigation, reversible triage, multi-account, thread view, compose via external editor, SMTP sending.

## Build & Development Commands

```bash
make build          # cargo build --release
make install        # install to ~/.local/bin/
make test           # cargo test
make clippy         # cargo clippy -- -W clippy::all
make check          # cargo check (fast type-checking)
make clean          # cargo clean
```

Single test: `cargo test test_name`

Debug logging: set `HUTT_LOG=/tmp/hutt.log` to enable file-based debug output (used by `debug_log!` and `mu_log!` macros).

## Architecture

### Event loop (`tui/mod.rs::run()`)
The core loop: wait for crossterm keyboard events → resolve via `KeyMapper` → dispatch `Action` in `App::handle_action()` → render via ratatui. Async compose/shell commands suspend the TUI and resume after.

### Input mode state machine (`keymap.rs`)
`InputMode` enum (Normal, Search, ThreadView, FolderPicker, CommandPalette, Help, SmartFolderCreate, SmartFolderName, MaildirCreate, MoveToFolder) controls which keybindings are active. `KeyMapper` resolves key events to `Action` variants, with config-driven overrides and g-prefix chord sequences.

### mu IPC (`mu_client.rs` + `mu_sexp.rs`)
`MuClient` spawns `mu server` as a child process, communicates via framed S-expressions over stdio. `mu_sexp.rs` handles the wire format (length-prefixed frames, comment/prompt skipping). The `lexpr` crate parses S-expressions into `Value`.

### Data model (`envelope.rs`)
`Envelope` is the core message representation (docid, msgid, from, to, cc, subject, date, flags, path, thread info). `Conversation` groups envelopes by thread for the conversations view. Flags use mu's symbol names (seen, replied, flagged, etc.).

### Config (`config.rs`)
TOML config at `~/.config/hutt/config.toml`. Multi-account: each account has name, email, maildir, smtp, folders (inbox/archive/drafts/sent/trash/spam), optional muhome, optional per-account sync_command. Global settings: editor, sync_command, conversations mode, snippets, keybindings.

### TUI widgets (`tui/` submodules)
Each widget is a separate module: `envelope_list` (message list), `preview` (message body), `thread_view` (conversation), `status_bar` (tab bar + bottom hints), `folder_picker`, `command_palette` (Ctrl+k fuzzy search), `help_overlay`.

### Key subsystems
- **Undo** (`undo.rs`): `UndoStack` tracks reversible triage (move, delete smart/maildir/split folder) for `z` key.
- **Compose** (`compose.rs`): Launches external editor, builds RFC 2822 messages. TUI suspends during editing.
- **SMTP** (`send.rs`): Sends via `lettre` with STARTTLS/SSL/OAuth2 support.
- **MIME rendering** (`mime_render.rs`): `RenderCache` caches rendered bodies keyed by (message_id, terminal_width). Uses `mail-parser` + `html2text`.
- **URI schemes** (`links.rs`): Accepts `mid:` (RFC 2392), `message:` (Apple Mail), `mailto:` (RFC 6068), and `hutt:` (app-specific search/navigate). Unix socket IPC at `$XDG_RUNTIME_DIR/hutt.sock`. All IPC commands accept optional `account` parameter. Legacy `hutt://` URLs accepted for backwards compatibility.
- **Smart folders** (`smart_folders.rs`): Saved mu queries, persisted as TOML in `~/.config/hutt/smart-folders/`.
- **Split inbox** (`splits.rs`): Inbox partitioning by query. Splits are persisted per-account as `~/.config/hutt/splits.<account>.toml`. Split queries run eagerly at startup/reindex, caching matched docids in `HashSet<u32>`. Inbox view excludes matched messages. `#` prefix in folder names.
- **Tab bar** (`tui/status_bar.rs` `TopBar`): Clickable folder tabs replacing the old top bar. Renders account badge, pinned inbox, scrollable tabs, overflow button. `TabRegion`/`TabRegionKind` structs enable mouse hit testing. Tab order configurable via `tabs` account config field with `/`, `#`, `@` wildcards.

### Mouse support
Crossterm mouse capture enabled during normal operation (disabled during compose/shell suspend). `MouseEventKind::Down` on row 0 checks `tab_regions` for tab/account/overflow clicks. Border drag (`dragging_border` + `list_pct`) resizes the list/preview split.

### Multi-account
`App::active_account` indexes into `config.accounts`. Each account can have its own mu database (`--muhome`). Account switching restarts the mu server. Smart folders, splits, and tabs are per-account. `InputMode::AccountPicker` provides a dropdown popup (triggered by `gA` or clicking the account badge).

## Conventions

- License: AGPL-3.0-or-later
- Error handling: `anyhow::Result` throughout
- Async runtime: Tokio multi-thread
- All triage actions must be reversible (push to `UndoStack`)
- Keybindings are config-driven with hardcoded defaults as fallback

## Tech Debt / Refactoring Notes

`tui/mod.rs` is ~2800 lines and growing. Not urgent but watch for these when next touching nearby code:

- **Extract rendering**: The draw closure in `run()` is ~200 lines mixing widget construction with inline popup rendering (e.g. AccountPicker uses local `use` imports of ratatui types). Could become a standalone `render()` function.
- **Generic list picker**: The popup input handling in `run()` repeats the same Down/Up/Enter/Esc pattern for FolderPicker, MoveToFolder, CommandPalette, and AccountPicker. Could be a shared list-navigation helper.
- **App sub-structs**: `App` has 60+ pub fields. Smart folder creation (5 fields), split state (4 fields), tab bar state (4 fields) are candidates for grouping into sub-structs.
- **Splits ↔ smart folders duplication**: Create, delete, and undo-restore flows for splits and smart folders are near-identical (~20 lines each). Could share a trait or helper if a third "virtual folder" type is added.
