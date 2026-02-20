# Using Hutt with Gmail / Google Workspace

This guide covers setting up mbsync, mu, and hutt to work with a
Gmail or Google Workspace account.

## 1. Gmail App Password

Gmail requires either an App Password or OAuth2 for IMAP/SMTP access.
App Passwords are simpler if your account (or Workspace admin) allows them.

1. Enable 2FA on your Google account if not already enabled.
2. Go to <https://myaccount.google.com/apppasswords>.
3. Create an app password for "Mail".
4. Store it somewhere secure. On macOS, the Keychain is convenient:

```sh
security add-generic-password -a you@example.com -s hutt-myaccount -w
# Paste the app password when prompted.

# Retrieval command (used in configs below):
security find-generic-password -a you@example.com -s hutt-myaccount -w
```

If your Workspace admin blocks app passwords, you'll need an OAuth2
helper like `mutt_oauth2.py` instead (not covered here).

## 2. mbsync Configuration

Add a channel to `~/.mbsyncrc`:

```ini
IMAPAccount gmail
Host imap.gmail.com
Port 993
User you@example.com
PassCmd "security find-generic-password -a you@example.com -s hutt-myaccount -w"
AuthMechs LOGIN
TLSType IMAPS

IMAPStore gmail-remote
Account gmail

MaildirStore gmail-local
Subfolders Verbatim
Path ~/Mail/gmail/
Inbox ~/Mail/gmail/Inbox

Channel gmail
Far :gmail-remote:
Near :gmail-local:
Patterns * !"[Gmail]/Important" !"[Gmail]/Starred"
Create Both
Expunge Both
SyncState *
```

### AuthMechs LOGIN

macOS ships a system SASL library that doesn't support PLAIN auth
with Gmail properly (you'll get a "SASL(-7)" error). Adding
`AuthMechs LOGIN` forces mbsync to use the LOGIN mechanism instead.

### Why include `[Gmail]/All Mail`?

Gmail's "archive" removes the Inbox label rather than moving to a
folder. The message stays in All Mail. If you exclude All Mail from
sync, archived messages disappear from your local Maildir and become
unsearchable in mu.

Including All Mail means every message exists locally twice (once in
its folder, once in All Mail). This costs disk space but keeps
archived mail searchable. mu handles deduplication in search results
reasonably well.

We exclude `[Gmail]/Important` and `[Gmail]/Starred` because they're
virtual folders based on flags, not useful as Maildir folders.

### Initial sync

```sh
mkdir -p ~/Mail/gmail
mbsync gmail
```

The first sync will take a while, especially for All Mail on large
accounts. Subsequent syncs are incremental.

## 3. Hutt Configuration

Add a second account to `~/.config/hutt/config.toml`:

```toml
[[accounts]]
name    = "gmail"
email   = "you@example.com"
maildir = "~/Mail/gmail"
sync_command = "mbsync gmail"

[accounts.smtp]
host             = "smtp.gmail.com"
port             = 465
encryption       = "ssl"
username         = "you@example.com"
password_command = "security find-generic-password -a you@example.com -s hutt-myaccount -w"

[accounts.folders]
inbox   = "/Inbox"
archive = "/[Gmail]/All Mail"
drafts  = "/[Gmail]/Drafts"
sent    = "/[Gmail]/Sent Mail"
trash   = "/[Gmail]/Trash"
spam    = "/[Gmail]/Spam"
```

If this is your only account, that's all you need. If you have
multiple accounts, you can set `default = true` on whichever account
should be selected at startup, and update the global sync command:

```toml
sync_command = "mbsync account1 gmail"
```

## 4. mu Database

Hutt auto-creates a per-account mu database at
`~/.cache/mu/<account_name>` on first use. No manual `mu init` is
needed.

If you want to set up the database manually:

```sh
mu init --muhome ~/.cache/mu/gmail --maildir ~/Mail/gmail
mu index --muhome ~/.cache/mu/gmail
```

## 5. Account Switching

With multiple accounts configured:

| Key           | Action           |
|---------------|------------------|
| `g Tab`       | Next account     |
| `g Shift+Tab` | Previous account |

The top bar shows `[account_name]` in cyan when multiple accounts are
configured.

## 6. Gmail Folder Mapping

Gmail's IMAP folders differ from standard IMAP. Here's how they map:

| Logical folder | Gmail IMAP path      | Notes                         |
|----------------|----------------------|-------------------------------|
| Inbox          | `/Inbox`             | Standard                      |
| Archive        | `/[Gmail]/All Mail`  | Gmail's "everything" folder   |
| Drafts         | `/[Gmail]/Drafts`    |                               |
| Sent           | `/[Gmail]/Sent Mail` |                               |
| Trash          | `/[Gmail]/Trash`     | Sometimes `/[Gmail]/Bin`      |
| Spam           | `/[Gmail]/Spam`      |                               |

The Trash folder name varies by locale. Check your Gmail IMAP settings
or run `mbsync -l gmail` to list available folders. UK English accounts
use `/[Gmail]/Bin` instead of `/[Gmail]/Trash`.

## Troubleshooting

**SASL(-7) error on sync**: Add `AuthMechs LOGIN` to the IMAPAccount
section in `.mbsyncrc`.

**"error moving" when archiving**: Make sure `[Gmail]/All Mail` is
included in your mbsync Patterns (not excluded with `!`), and that
you've synced it at least once.

**Archived messages not searchable**: You're probably not syncing
`[Gmail]/All Mail`. See the "Why include All Mail" section above.

**Slow initial sync**: The first sync of All Mail can take a long time
for large accounts. It runs in the background if triggered from hutt
(`G` key). Subsequent syncs are fast.
