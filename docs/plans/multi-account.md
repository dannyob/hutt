# Multi-Account Support

## Status: Implemented

## What Was Done

All steps from the original design have been implemented:

1. **Config changes** — `AccountConfig` gained `muhome`, `default`, and
   per-account `sync_command`. Helpers: `default_account_index()`,
   `effective_sync_command()`, `effective_muhome()` with auto-derivation
   to `~/.cache/mu/<account_name>` for multi-account setups.

2. **MuClient --muhome** — `MuClient::start()` accepts an optional muhome
   path and passes `--muhome` to the mu server process.

3. **mu init auto-setup** — `ensure_mu_database()` checks for the xapian
   directory and runs `mu init` + `mu index` if missing.

4. **Active account tracking** — `App.active_account` index + `account()`
   helper. All `.accounts.first()` calls replaced.

5. **Per-account smart folders** — Stored in
   `smart_folders.<account_name>.toml`. Falls back to plain
   `smart_folders.toml` for migration.

6. **Account switching** — `NextAccount`/`PrevAccount` actions bound to
   `g Tab` / `g Shift+Tab`. `switch_account()` quits mu, inits new db
   if needed, starts new server, clears state, reloads smart folders,
   navigates to inbox.

7. **UI account indicator** — Top bar shows `[AccountName]` in cyan when
   multiple accounts are configured.

8. **Per-account sync** — `SyncMail` uses `effective_sync_command()`.

9. **Compose From address** — Uses active account's email and SMTP config.

10. **Startup** — Uses `default_account_index()` and account's inbox folder.

11. **IPC compose fix** — `ComposePending` enum with `Kind` and `Ready`
    variants preserves pre-set To/Subject from `hutt://compose` URLs.

## Resolved Questions

- **Smart folders**: Per-account (separate files per account name).
- **Account switching keybinding**: `g Tab` / `g Shift+Tab` (wraps around).
- **muhome auto-derivation**: Yes, `~/.cache/mu/<account_name>` when
  multiple accounts and no explicit muhome.
- **Auto mu init**: Yes, `ensure_mu_database()` handles it at startup
  and on account switch.
