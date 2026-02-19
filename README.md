# hutt

A fast, keyboard-driven TUI email client for the terminal, inspired by
Superhuman's UX. Built in Rust with [ratatui](https://ratatui.rs/).

hutt uses [mu](https://www.djcbsoftware.nl/code/mu/) as its mail indexing
backend and expects a Maildir-synced mailbox (via
[mbsync](https://isstracked.com/isstracked.com/isync),
[offlineimap](https://www.offlineimap.org/), or similar).

## Features

- **Vim-style navigation** — j/k, gg/G, Ctrl+d/u, and more
- **Fast triage** — archive, trash, spam, toggle read/star with single keys
- **Undo** — reversible triage actions with `z`
- **Multi-select** — bulk-select messages with x/J/K, then triage all at once
- **Search** — full mu query syntax via `/`
- **Quick filters** — toggle unread (U), starred (S), needs-reply (R)
- **Folder switching** — `gi` for inbox, `ga` for archive, `gl` for picker, etc.
- **Thread view** — expand/collapse messages in a conversation
- **Compose** — new messages, reply, reply-all, forward via your `$EDITOR`
- **SMTP sending** — send mail directly from the TUI via STARTTLS/TLS/plain
- **Linkability** — `hutt://` URL scheme, copy message URLs, open in browser
- **Command palette** — Ctrl+k to fuzzy-search all available actions
- **Help overlay** — press `?` for a full shortcut reference

## Requirements

- [mu](https://www.djcbsoftware.nl/code/mu/) (tested with mu 1.10+)
- A Maildir mailbox synced by mbsync, offlineimap, or similar
- Rust toolchain (for building)

## Installation

```sh
git clone https://github.com/your/hutt.git
cd hutt
cargo build --release
cp target/release/hutt ~/.local/bin/
```

## Configuration

hutt looks for its config file at:

1. `$HUTT_CONFIG`
2. `$XDG_CONFIG_HOME/hutt/config.toml`
3. `~/.config/hutt/config.toml`

If no config file is found, hutt starts with sensible defaults (nvim as
editor, /Inbox as starting folder).

See [config.sample.toml](config.sample.toml) for a full annotated example.
The minimum useful config:

```toml
editor = "nvim"
sync_command = "mbsync -a"

[[accounts]]
name = "Personal"
email = "you@example.com"
maildir = "~/Mail/personal"

[accounts.smtp]
host = "smtp.example.com"
port = 587
encryption = "starttls"
username = "you@example.com"
password_command = "pass show email/smtp"
```

## Usage

```sh
hutt              # opens /Inbox
hutt /Sent        # opens a specific folder
```

## Keyboard Shortcuts

Press `?` inside hutt for the full interactive reference. Press `Ctrl+k`
to open the command palette and fuzzy-search any action.

### Navigation

| Key            | Action                    |
|----------------|---------------------------|
| `j` / `Down`   | Move down                 |
| `k` / `Up`     | Move up                   |
| `gg`           | Jump to top               |
| `G`            | Jump to bottom            |
| `Space`        | Scroll preview down       |
| `Shift+Space`  | Scroll preview up         |
| `Ctrl+d`       | Half page down            |
| `Ctrl+u`       | Half page up              |

### Triage

| Key | Action              |
|-----|---------------------|
| `e` | Archive             |
| `#` | Trash               |
| `!` | Mark as spam        |
| `u` | Toggle read/unread  |
| `s` | Toggle star         |
| `z` | Undo last action    |

### Folders

| Key  | Action          |
|------|-----------------|
| `gi` | Go to Inbox     |
| `ga` | Go to Archive   |
| `gd` | Go to Drafts    |
| `gt` | Go to Sent      |
| `g#` | Go to Trash     |
| `g!` | Go to Spam      |
| `gl` | Folder picker   |

### Search & Filters

| Key | Action               |
|-----|----------------------|
| `/` | Search (mu query)    |
| `U` | Toggle unread filter |
| `S` | Toggle starred filter|
| `R` | Toggle needs-reply   |

### Selection

| Key | Action                   |
|-----|--------------------------|
| `x` | Toggle select            |
| `J` | Select + move down       |
| `K` | Select + move up         |

Triage actions (e, #, !, u, s) apply to all selected messages when a
selection is active.

### Thread View

| Key          | Action               |
|--------------|----------------------|
| `Enter`      | Open thread          |
| `j` / `k`    | Navigate messages    |
| `o`          | Toggle expand        |
| `O`          | Expand/collapse all  |
| `q` / `Esc`  | Close thread         |

Triage and compose keys work in thread view too.

### Compose

| Key | Action     |
|-----|------------|
| `c` | Compose    |
| `r` | Reply      |
| `a` | Reply all  |
| `f` | Forward    |

Opens your configured editor. Save and quit to send; quit without saving
to cancel.

### Links & Clipboard

| Key      | Action              |
|----------|---------------------|
| `y`      | Copy message URL    |
| `Y`      | Copy thread URL     |
| `Ctrl+o` | Open in browser     |

### Other

| Key      | Action            |
|----------|-------------------|
| `Ctrl+k` | Command palette  |
| `Ctrl+r` | Sync mail        |
| `?`      | Help overlay      |
| `q`      | Quit              |

## Neovim Plugin

hutt includes an optional Neovim plugin for compose mode. Add the `nvim/`
directory to your runtime path:

```lua
-- lazy.nvim
{ dir = "~/path/to/hutt/nvim" }

-- or in init.lua
vim.opt.rtp:append("~/path/to/hutt/nvim")
require("hutt").setup({
  -- signature = "Best,\nYour Name",
})
```

The plugin provides:
- `filetype=mail` with textwidth=72 and spell checking for compose buffers
- `:HuttSend` / `<leader>s` to save and send
- `:HuttDiscard` / `<leader>d` to cancel
- Contact completion via `mu cfind` on To/Cc/Bcc lines (Ctrl+x Ctrl+o)

## macOS URL Scheme

To register the `hutt://` URL scheme on macOS:

```sh
cp -r macos/hutt-opener ~/Applications/"Hutt Opener.app"
open ~/Applications/"Hutt Opener.app"
```

This allows clicking `hutt://message/<id>` links to open the corresponding
message in a running hutt instance.

## Architecture

```
src/
├── main.rs           Entry point, arg parsing
├── config.rs         TOML config loading
├── mu_client.rs      mu server IPC (S-expression protocol)
├── mu_sexp.rs        S-expression parser
├── envelope.rs       Envelope data model, flag handling
├── mime_render.rs    MIME parsing and text rendering
├── keymap.rs         Input mode state machine, key mapping
├── compose.rs        Compose context building, editor launch
├── send.rs           SMTP sending via lettre
├── links.rs          URL scheme, clipboard, IPC socket
├── undo.rs           Undo stack for triage actions
└── tui/
    ├── mod.rs            App state, action dispatch, main loop
    ├── envelope_list.rs  Message list widget
    ├── preview.rs        Message preview pane
    ├── status_bar.rs     Top bar (folder/counts) and bottom bar (hints)
    ├── thread_view.rs    Thread conversation widget
    ├── folder_picker.rs  Folder picker popup
    ├── command_palette.rs Command palette popup
    └── help_overlay.rs   Keyboard shortcut reference
```

## License

AGPL-3.0-or-later
