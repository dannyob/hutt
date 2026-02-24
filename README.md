# hutt

A fast, keyboard-driven TUI email client for the terminal, inspired by
Superhuman's UX. Built in Rust with [ratatui](https://ratatui.rs/).

hutt uses [mu](https://www.djcbsoftware.nl/code/mu/) as its mail indexing
backend and expects a Maildir-synced mailbox (via
[mbsync](https://isync.sourceforge.io/),
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
- **Conversations mode** — group messages by thread in the message list
- **Split inbox** — partition your inbox by query (e.g. GitHub, newsletters, VIPs)
- **Smart folders** — saved mu searches as virtual folders
- **Multi-account** — switch between accounts with `gTab` or the tab bar
- **Tab bar** — clickable folder tabs with mouse support
- **Mouse support** — click tabs to navigate, drag border to resize panes
- **Compose** — new messages, reply, reply-all, forward via your `$EDITOR`
- **SMTP sending** — send mail directly from the TUI via STARTTLS/TLS/plain
- **Linkability** — `mid:`, `message:`, `mailto:`, `hutt:` URI schemes; IPC; copy message URLs
- **Command palette** — Ctrl+k to fuzzy-search all available actions
- **Help overlay** — press `?` for a full shortcut reference
- **Scriptable CLI** — `hutt remote` with `--sexp`/`--json` output for scripting
- **mu server proxy** — `hutt server` as a drop-in `mu server` replacement

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
hutt                              # opens default account inbox
hutt /Sent                        # opens a specific folder
hutt -a work /Drafts              # opens Drafts on the 'work' account
```

See `hutt --help` for full CLI documentation.

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
| `Ctrl+f`       | Full page down            |
| `Ctrl+b`       | Full page up              |

### Triage

| Key | Action              |
|-----|---------------------|
| `e` | Archive             |
| `#` | Trash               |
| `!` | Mark as spam        |
| `u` | Toggle read/unread  |
| `s` | Toggle star         |
| `z` | Undo last action    |

### Folders & Tabs

| Key            | Action              |
|----------------|---------------------|
| `gi`           | Go to Inbox         |
| `ga`           | Go to Archive       |
| `gd`           | Go to Drafts        |
| `gt`           | Go to Sent          |
| `gs`           | Go to Starred       |
| `g*`           | Go to All Mail      |
| `g#`           | Go to Trash         |
| `g!`           | Go to Spam          |
| `gl`           | Folder picker       |
| `Tab`          | Next tab            |
| `Shift+Tab`    | Previous tab        |
| `gA`           | Account picker      |
| `gTab`         | Next account        |
| `gShift+Tab`   | Previous account    |
| `Ctrl+1-9`     | Switch to account N |

### Search & Filters

| Key | Action               |
|-----|----------------------|
| `/` | Search (mu query)    |
| `U` | Toggle unread filter |
| `S` | Toggle starred filter|
| `R` | Toggle needs-reply   |

### Selection

| Key              | Action                   |
|------------------|--------------------------|
| `x`              | Toggle select            |
| `Cmd+a` / `Ctrl+a` | Select all (toggle)   |
| `Esc`            | Clear selection          |
| `J`              | Select + move down       |
| `K`              | Select + move up         |

Triage actions (e, #, !, u, s) apply to all selected messages when a
selection is active.

### Thread View

| Key          | Action               |
|--------------|----------------------|
| `Enter`          | Open thread          |
| `j`/`k`/`n`/`p` | Navigate messages    |
| `o`              | Toggle expand        |
| `O`              | Expand/collapse all  |
| `q` / `Esc`      | Close thread         |

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

## Split Inbox

Split inbox partitions your inbox into focused sub-views using mu
queries — similar to Superhuman's split inbox feature. Each split
removes matching messages from the main Inbox view and shows them in
their own tab.

Splits are defined per-account in `~/.config/hutt/splits.<account>.toml`:

```toml
[[splits]]
name = "GitHub"
query = "from:notifications@github.com"

[[splits]]
name = "Newsletters"
query = "from:substack.com or from:noreply@medium.com"

[[splits]]
name = "Starred"
query = "flag:flagged"
```

Split tabs appear in the tab bar with a `#` prefix (e.g. `#GitHub`).
You can also create and delete splits from within hutt:

- **Create**: `Ctrl+k` → "Create Split", or use the command palette
- **Delete**: open the folder picker (`gl`), navigate to a `#split`,
  press `d`
- **Undo delete**: press `z`

Splits use client-side filtering — queries run at startup and after
each reindex, caching matching message IDs. The inbox view excludes
anything that matches any split. Splits may overlap; a message can
appear in multiple splits.

### Importing from Superhuman

If you're migrating from Superhuman, the included import script can
extract your split definitions:

```sh
# Show all splits from all accounts
python3 scripts/superhuman-import.py

# Export as hutt TOML for a specific account
python3 scripts/superhuman-import.py --account user@example.com --hutt

# Include disabled splits too
python3 scripts/superhuman-import.py --include-disabled --hutt
```

## Smart Folders

Smart folders are saved mu searches that appear as virtual folders.
They're stored per-account in `~/.config/hutt/smart-folders/<account>/`.

Smart folder tabs appear with an `@` prefix (e.g. `@Unread today`).

- **Create**: `Ctrl+k` → "Create Smart Folder"
- **Delete**: folder picker (`gl`) → navigate to `@folder` → press `d`

## Multi-Account

Configure multiple accounts in your config file:

```toml
[[accounts]]
name = "work"
email = "you@work.com"
maildir = "~/Mail/work"
default = true
# ...

[[accounts]]
name = "personal"
email = "you@example.com"
maildir = "~/Mail/personal"
# ...
```

Switch accounts with:
- `gA` — open account picker popup
- `gTab` / `gShift+Tab` — cycle to next/previous account
- Click the account name in the tab bar

Each account has its own mu database, folders, splits, and smart
folders. Set `muhome` per-account if they use separate mu databases.

## Tab Bar

The top bar shows clickable folder tabs:

```
 work  /Inbox  #GitHub  #Newsletters  /Archive  /Sent  @Unread  …
```

- **Account badge** (left) — click to open account picker
- **Inbox** — always pinned on the left
- **Folder tabs** — click to navigate; `Tab`/`Shift+Tab` to cycle
- **Overflow `…`** (right) — click to open the full folder picker

Tabs are color-coded: splits (`#`) in cyan, smart folders (`@`) in
yellow, maildir folders (`/`) in white. The selected tab is highlighted
in blue.

### Configuring Tab Order

Customize which tabs appear and in what order per-account:

```toml
[[accounts]]
name = "work"
tabs = ["/Inbox", "#GitHub", "#Newsletters", "#", "/Archive", "/Sent", "@"]
# ...
```

Wildcards expand to "remaining items of this type":
- `"/"` — all remaining maildir folders not explicitly listed
- `"#"` — all remaining splits not explicitly listed
- `"@"` — all remaining smart folders not explicitly listed

Default when `tabs` is omitted: `["/Inbox", "#", "/", "@"]`

## Mouse Support

hutt supports mouse interaction:

- **Tab bar** — click any tab to navigate to that folder
- **Account badge** — click to open the account picker
- **Overflow `…`** — click to open the folder picker
- **Border drag** — drag the border between the message list and
  preview pane to resize (hold left click and drag)

## Custom Keybindings

All default keybindings can be overridden or extended in the `[bindings]`
section of your config file. Values can be:

- A built-in action name: `"archive"`, `"trash"`, `"sync_mail"`, etc.
- A folder path (starts with `/`): `"/Sent"`, `"/Archive/2026"`
- A shell command table: `{ shell = "mbsync -a", reindex = true }`

```toml
[bindings]
G         = { shell = "mbsync -a", reindex = true }
"ctrl+t"  = { shell = "tig", suspend = true }
"g s"     = "/Sent"
A         = "archive"
```

Use `[bindings.normal]` and `[bindings.thread]` for per-mode overrides
(e.g., bind `o` to different actions in list vs thread view).

Key syntax: `"e"`, `"#"`, `"G"` (shift), `"ctrl+r"`, `"shift+space"`,
`"g i"` (two-key sequence), `"enter"`, `"esc"`, `"space"`, `"f1"`–`"f12"`.

Shell commands run asynchronously by default. Add `suspend = true` for
interactive programs that need the terminal (the TUI pauses and resumes
afterwards). Add `reindex = true` to re-index mu and reload the folder
after the command finishes.

See [config.sample.toml](config.sample.toml) for the full list of action
names.

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

---

## Remote Commands

When hutt is running, you can control it from another terminal via
`hutt remote` (or `hutt r`):

```sh
hutt r search from:alice              # search in the running instance
hutt r search --account=work from:bob # search on a specific account
hutt r navigate /Sent                 # switch to a folder
hutt r open <message-id>              # open a message by Message-ID
hutt r thread <message-id>            # open a thread by Message-ID
hutt r compose --to=bob@example.com --subject="Hello"
hutt r open-url 'mid:abc@example.com?view=thread'
hutt r quit                           # quit the running instance
```

All remote commands accept `--account=NAME` to target a specific account.

### Structured Output (`--sexp`, `--json`)

Remote commands can return structured data for scripting. By default
output is silent (the command executes in the TUI only). Add `--sexp`
or `--json` to get the results on stdout.

**`--sexp`** — one mu-compatible S-expression plist per line:

```sh
$ hutt r --sexp search 'from:alice date:today'
(:docid 42 :subject "Project update" :path "/mail/Inbox/cur/123:2,S" :from ((:email "alice@example.com" :name "Alice")) :date (27028 6999 0) :flags (seen) :maildir "/Inbox")
(:docid 43 :subject "Re: Project update" :path "/mail/Inbox/cur/124:2,S" ...)
```

**`--json`** — one JSON object per line (ndjson), with ISO 8601 dates:

```sh
$ hutt r --json search 'from:alice date:today'
{"docid":42,"subject":"Project update","path":"/mail/Inbox/cur/123:2,S","from":[{"email":"alice@example.com","name":"Alice"}],"date":"2026-02-24T10:30:00+00:00","flags":["seen"],"maildir":"/Inbox"}
```

**`--wrapped`** — combine with `--sexp` or `--json` to wrap all results
in a single container:

```sh
# Wrapped sexp (mu-compatible response format)
$ hutt r --sexp --wrapped search 'from:alice'
(:headers ((:docid 42 ...) (:docid 43 ...)) :found 2)

# Wrapped JSON
$ hutt r --json --wrapped search 'from:alice'
{"headers":[{...},{...}],"found":2}
```

**Errors** are also structured (exit code 1):

```sh
$ hutt r --json open nonexistent@example.com
{"error":"message not found: nonexistent@example.com"}
```

**Scripting examples:**

```sh
# Get file paths for all messages from alice today
hutt r --json search 'from:alice date:today' | jq -r '.path'

# Count unread messages
hutt r --json --wrapped search 'flag:unread' | jq '.found'

# Pipe message files into another tool
hutt r --json search 'flag:attach' | jq -r '.path' | xargs -I{} mshow {}
```

### What each command returns

| Command | `--sexp` / `--json` output |
|---------|---------------------------|
| `search <query>` | All matching envelopes |
| `open <message-id>` | The matched envelope |
| `thread <message-id>` | All envelopes in the thread |
| `navigate <folder>` | All envelopes in the folder |
| `compose`, `quit` | Nothing (just ok/error) |

Each envelope includes: `docid`, `message-id`, `subject`, `from`, `to`,
`date`, `flags`, `path` (full filesystem path to the Maildir message
file), `maildir`, and thread metadata.

## hutt server

`hutt server` is a drop-in replacement for `mu server` that proxies
commands through the running hutt instance's mu server via IPC. This
solves the Xapian exclusive lock problem — external tools can query mu
while hutt is running.

```sh
hutt server                              # interactive mode (stdin/stdout)
hutt server --eval '(ping)'             # evaluate a single S-expression
hutt server --eval '(find :query "flag:unread" :sortfield :date :maxnum 100 :threads t)'
hutt server --muhome ~/.cache/mu/work   # route to a specific account
hutt server --account work              # same, by account name
hutt server --commands                  # list available mu commands
```

When hutt is running, commands are proxied through its mu server. When
hutt is not running (or `--muhome` doesn't match any account), `hutt
server` falls back to spawning `mu server` directly — making it a safe
drop-in replacement in all contexts.

**Interactive mode** speaks the same mu wire protocol (length-prefixed
S-expression frames on stdin/stdout), so it works as a backend for
mu4e, mu-cite, or any tool that expects `mu server`.

**`--eval` mode** sends a single S-expression and prints the response
frames, then exits.

## URI Schemes

hutt uses standard URI schemes where they exist, with app-specific schemes
only for operations no standard covers. hutt must be running for IPC
(it listens on a Unix domain socket at `$XDG_RUNTIME_DIR/hutt.sock` or
`/tmp/hutt-<uid>.sock`).

### Standard schemes

| URI                                     | Standard    | Action                  |
|-----------------------------------------|-------------|-------------------------|
| `mid:<message-id>`                      | RFC 2392    | Open a message          |
| `mid:<message-id>?view=thread`          | RFC 2392    | Open a thread           |
| `message:<message-id>`                  | IANA prov.  | Open a message (Apple Mail) |
| `mailto:addr?subject=text`              | RFC 6068    | Compose                 |

### App-specific schemes

| URI                                              | Action                  |
|--------------------------------------------------|-------------------------|
| `hutt:search?q=<query>[&account=<name>]`         | Run a search            |
| `hutt:navigate?folder=<path>[&account=<name>]`   | Switch to a folder      |

The `account` parameter is optional — omit it to operate on the active
account. For `mid:` and `message:` URLs, Message-IDs are globally unique
(RFC 2822), so hutt searches all accounts.

Legacy `hutt://` URLs (with `//`) are still accepted for backwards
compatibility.

### Copy to clipboard

`y u` copies the current message's `mid:` URL. `y t` copies the thread URL.

### macOS

```sh
make install-macos-handler
```

Installs a handler app for `mid:`, `message:`, and `hutt:` schemes.

### Linux (freedesktop / GNOME / KDE)

```sh
make install-linux-handler
```

Installs `hutt-open` to `~/.local/bin/` and registers a `.desktop` file
with `xdg-mime` as the handler for `mid`, `message`, and `hutt` schemes.

## Debugging

Set `HUTT_LOG` to a file path for debug output:

```sh
HUTT_LOG=/tmp/hutt.log hutt
```

## Architecture

```
src/
├── main.rs           Entry point, arg parsing, hutt server CLI
├── config.rs         TOML config loading
├── mu_client.rs      mu server IPC (S-expression protocol, find_capturing)
├── mu_sexp.rs        S-expression parser, sexp↔JSON conversion, wire framing
├── envelope.rs       Envelope data model, flag handling
├── mime_render.rs    MIME parsing and text rendering
├── keymap.rs         Input mode state machine, key mapping
├── compose.rs        Compose context building, editor launch
├── send.rs           SMTP sending via lettre
├── links.rs          URL schemes, clipboard, bidirectional IPC (IpcCommand/IpcResponse)
├── undo.rs           Undo stack for triage actions
├── splits.rs         Split inbox persistence (per-account TOML)
├── smart_folders.rs  Smart folder persistence
└── tui/
    ├── mod.rs            App state, action dispatch, main loop
    ├── envelope_list.rs  Message list widget
    ├── preview.rs        Message preview pane
    ├── status_bar.rs     Tab bar, bottom status bar
    ├── thread_view.rs    Thread conversation widget
    ├── folder_picker.rs  Folder picker popup
    ├── command_palette.rs Command palette popup
    └── help_overlay.rs   Keyboard shortcut reference
scripts/
└── superhuman-import.py  Extract split inbox config from Superhuman
macos/
├── hutt-opener.applescript   AppleScript URL event handler
└── hutt-opener/Contents/     .app bundle template (Info.plist + shell script)
linux/
└── hutt-opener.desktop       XDG URL scheme registration
```

### IPC protocol

hutt's IPC is bidirectional request/response over a Unix domain socket.
The client sends a JSON-encoded `IpcCommand`, shuts down the write side,
and reads back a JSON-encoded `IpcResponse`:

```
IpcCommand: Open | Navigate | Quit | MuCommand
IpcResponse: Ok | Error { message } | MuFrames { frames: [sexp strings] }
```

`MuFrames` carries raw mu S-expression strings — either individual
envelope plists (for remote commands) or raw mu server response frames
(for `hutt server` proxying). The CLI formats these as `--sexp` or
`--json` output.

## License

AGPL-3.0-or-later
