use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub accounts: Vec<AccountConfig>,
    pub editor: String,
    pub sync_command: Option<String>,
    pub snippets: Vec<Snippet>,
    #[serde(default)]
    pub bindings: BindingsSection,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            editor: "nvim".to_string(),
            sync_command: None,
            snippets: Vec::new(),
            bindings: BindingsSection::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct AccountConfig {
    pub name: String,
    pub email: String,
    pub maildir: String,
    pub smtp: SmtpConfig,
    #[serde(default)]
    pub folders: FolderConfig,
    /// Path to mu database directory. Auto-derived for multi-account setups.
    pub muhome: Option<String>,
    /// Mark this account as the default (first with default=true wins).
    #[serde(default)]
    pub default: bool,
    /// Per-account sync command (overrides global sync_command).
    pub sync_command: Option<String>,
}

// ---------------------------------------------------------------------------
// SMTP
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// One of "starttls", "ssl", "none".
    pub encryption: String,
    pub username: String,
    pub password: Option<String>,
    /// Shell command whose stdout provides the password (e.g. "pass email/work").
    pub password_command: Option<String>,
    /// OAuth2 access-token command, if used instead of password auth.
    pub oauth2_command: Option<String>,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 587,
            encryption: "starttls".to_string(),
            username: String::new(),
            password: None,
            password_command: None,
            oauth2_command: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Folder mapping
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct FolderConfig {
    pub inbox: String,
    pub archive: String,
    pub drafts: String,
    pub sent: String,
    pub trash: String,
    pub spam: String,
}

impl Default for FolderConfig {
    fn default() -> Self {
        Self {
            inbox: "/Inbox".to_string(),
            archive: "/Archive".to_string(),
            drafts: "/Drafts".to_string(),
            sent: "/Sent".to_string(),
            trash: "/Trash".to_string(),
            spam: "/Spam".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Snippets  (templates triggered by a prefix while composing)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Snippet {
    pub trigger: String,
    pub body: String,
}

// ---------------------------------------------------------------------------
// Keybindings
// ---------------------------------------------------------------------------

/// What a key binding maps to.
///
/// Strings are shorthand: a bare name like `"archive"` is a built-in action,
/// a `/`-prefixed string like `"/Sent"` navigates to that folder.
///
/// A table with `shell = "..."` runs a shell command, with optional
/// `reindex` (re-index mu afterwards) and `suspend` (pause TUI for
/// interactive programs).
///
/// What a key binding maps to.
///
/// Strings are shorthand: a bare name like `"archive"` is a built-in action,
/// a `/`-prefixed string like `"/Sent"` navigates to that folder.
///
/// A table with `shell = "..."` runs a shell command.
/// A table with `move = "..."` moves selected messages to a folder
/// (alias like `"archive"` or literal path like `"/Projects"`).
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum BindingValue {
    /// `"archive"` (action name) or `"/Sent"` (folder path).
    Short(String),
    /// `{ shell = "mbsync almnck", reindex = true, suspend = false }`.
    Shell {
        shell: String,
        #[serde(default)]
        reindex: bool,
        #[serde(default)]
        suspend: bool,
    },
    /// `{ move = "/Projects" }` or `{ move = "archive" }`.
    Move {
        #[serde(rename = "move")]
        folder: String,
    },
}

/// The `[bindings]` config section.
///
/// Top-level keys are global (apply to normal + thread modes).
/// `[bindings.normal]` and `[bindings.thread]` provide per-mode overrides.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct BindingsSection {
    /// Mode-specific bindings for normal (list) mode.
    #[serde(default)]
    pub normal: HashMap<String, BindingValue>,
    /// Mode-specific bindings for thread view mode.
    #[serde(default)]
    pub thread: HashMap<String, BindingValue>,
    /// Global bindings (apply to both normal and thread modes).
    #[serde(flatten)]
    pub global: HashMap<String, BindingValue>,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl Config {
    /// Return the index of the default account: first with `default = true`, or 0.
    pub fn default_account_index(&self) -> usize {
        self.accounts
            .iter()
            .position(|a| a.default)
            .unwrap_or(0)
    }

    /// Return the effective sync command for an account index.
    /// Uses the account's sync_command if set, otherwise falls back to global.
    pub fn effective_sync_command(&self, account_idx: usize) -> Option<&str> {
        self.accounts
            .get(account_idx)
            .and_then(|a| a.sync_command.as_deref())
            .or(self.sync_command.as_deref())
    }

    /// Return the effective muhome for an account.
    ///
    /// If the account has an explicit `muhome`, use it (expanding `~`).
    /// If there are multiple accounts and no explicit muhome, auto-derive
    /// as `~/.cache/mu/<account_name>`.
    /// Single-account configs without muhome return None (use system default).
    pub fn effective_muhome(&self, account_idx: usize) -> Option<String> {
        let account = self.accounts.get(account_idx)?;
        if let Some(ref muhome) = account.muhome {
            Some(expand_tilde(muhome))
        } else if self.accounts.len() > 1 {
            let home = std::env::var("HOME").unwrap_or_default();
            Some(format!("{}/.cache/mu/{}", home, account.name))
        } else {
            None
        }
    }

    /// Try to load the configuration file from, in order:
    ///
    /// 1. `$HUTT_CONFIG`
    /// 2. `$XDG_CONFIG_HOME/hutt/config.toml`
    /// 3. `~/.config/hutt/config.toml`
    ///
    /// If none of these paths exist, return a default `Config`.
    pub fn load() -> Result<Self> {
        if let Some(path) = Self::locate() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read config file {}", path.display()))?;
            let config: Config = toml::from_str(&contents)
                .with_context(|| format!("failed to parse config file {}", path.display()))?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Return the first config path that actually exists on disk, or `None`.
    fn locate() -> Option<PathBuf> {
        let candidates = Self::candidate_paths();
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Ordered list of paths we check for a config file.
    fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // 1. $HUTT_CONFIG
        if let Ok(p) = std::env::var("HUTT_CONFIG") {
            paths.push(PathBuf::from(p));
        }

        // 2. $XDG_CONFIG_HOME/hutt/config.toml
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            paths.push(PathBuf::from(xdg).join("hutt").join("config.toml"));
        }

        // 3. ~/.config/hutt/config.toml
        if let Ok(home) = std::env::var("HOME") {
            paths.push(
                PathBuf::from(home)
                    .join(".config")
                    .join("hutt")
                    .join("config.toml"),
            );
        }

        paths
    }
}

/// Expand `~/` prefix in a path string.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/{}", home, rest)
    } else {
        path.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = Config::default();
        assert_eq!(cfg.editor, "nvim");
        assert!(cfg.accounts.is_empty());
        assert!(cfg.sync_command.is_none());
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
            editor = "emacs"
            sync_command = "mbsync -a"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.editor, "emacs");
        assert_eq!(cfg.sync_command.as_deref(), Some("mbsync -a"));
        assert!(cfg.accounts.is_empty());
    }

    #[test]
    fn parse_full_account() {
        let toml_str = r#"
            [[accounts]]
            name = "Work"
            email = "danny@example.com"
            maildir = "~/Maildir/work"

            [accounts.smtp]
            host = "smtp.example.com"
            port = 465
            encryption = "ssl"
            username = "danny@example.com"
            password_command = "pass email/work"

            [accounts.folders]
            inbox = "/INBOX"
            archive = "/All Mail"
            drafts = "/Drafts"
            sent = "/Sent Mail"
            trash = "/Bin"
            spam = "/Junk"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.accounts.len(), 1);

        let acct = &cfg.accounts[0];
        assert_eq!(acct.name, "Work");
        assert_eq!(acct.smtp.host, "smtp.example.com");
        assert_eq!(acct.smtp.port, 465);
        assert_eq!(acct.smtp.encryption, "ssl");
        assert_eq!(
            acct.smtp.password_command.as_deref(),
            Some("pass email/work")
        );
        assert_eq!(acct.folders.inbox, "/INBOX");
        assert_eq!(acct.folders.trash, "/Bin");
    }

    #[test]
    fn parse_snippets() {
        let toml_str = r#"
            [[snippets]]
            trigger = "/sig"
            body = "Best,\nDanny"

            [[snippets]]
            trigger = "/ty"
            body = "Thanks for your email."
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.snippets.len(), 2);
        assert_eq!(cfg.snippets[0].trigger, "/sig");
    }

    #[test]
    fn parse_bindings_global() {
        let toml_str = r#"
            [bindings]
            A = "archive"
            "g s" = "/Sent"
            G = { shell = "mbsync almnck", reindex = true }
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.bindings.global.len(), 3);
        assert!(matches!(
            cfg.bindings.global.get("A"),
            Some(BindingValue::Short(s)) if s == "archive"
        ));
        assert!(matches!(
            cfg.bindings.global.get("g s"),
            Some(BindingValue::Short(s)) if s == "/Sent"
        ));
        assert!(matches!(
            cfg.bindings.global.get("G"),
            Some(BindingValue::Shell { shell, reindex: true, suspend: false })
                if shell == "mbsync almnck"
        ));
    }

    #[test]
    fn parse_bindings_per_mode() {
        let toml_str = r#"
            [bindings]
            e = "archive"

            [bindings.normal]
            o = "open_thread"

            [bindings.thread]
            o = "thread_toggle_expand"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.bindings.global.len(), 1);
        assert!(matches!(
            cfg.bindings.normal.get("o"),
            Some(BindingValue::Short(s)) if s == "open_thread"
        ));
        assert!(matches!(
            cfg.bindings.thread.get("o"),
            Some(BindingValue::Short(s)) if s == "thread_toggle_expand"
        ));
    }

    #[test]
    fn parse_bindings_shell_suspend() {
        let toml_str = r#"
            [bindings]
            "ctrl+t" = { shell = "tig", suspend = true }
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            cfg.bindings.global.get("ctrl+t"),
            Some(BindingValue::Shell { shell, reindex: false, suspend: true })
                if shell == "tig"
        ));
    }

    #[test]
    fn parse_real_config_with_bindings() {
        let toml_str = r#"
            editor = "nvim"

            [[accounts]]
            name    = "almnck"
            email   = "olddanny@almnck.com"
            maildir = "~/Private/almnck-mail"

            [accounts.smtp]
            host             = "smtp.purelymail.com"
            port             = 465
            encryption       = "ssl"
            username         = "olddanny@almnck.com"
            password_command = "pass email/work"

            [accounts.folders]
            inbox   = "/Inbox"
            archive = "/Archive"
            drafts  = "/Drafts"
            sent    = "/Sent"
            trash   = "/Trash"
            spam    = "/Junk"

            [bindings]
            G = { shell = "mbsync almnck", reindex = true }
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.accounts.len(), 1, "accounts should have 1 entry");
        assert_eq!(cfg.accounts[0].name, "almnck");
        assert_eq!(cfg.accounts[0].email, "olddanny@almnck.com");
        assert_eq!(cfg.accounts[0].maildir, "~/Private/almnck-mail");
        assert_eq!(cfg.accounts[0].smtp.host, "smtp.purelymail.com");
        assert_eq!(cfg.accounts[0].folders.inbox, "/Inbox");
        assert_eq!(cfg.bindings.global.len(), 1);
        assert!(cfg.bindings.global.contains_key("G"));
    }

    #[test]
    fn parse_config_without_bindings() {
        // Same config but WITHOUT [bindings] — the way it used to work
        let toml_str = r#"
            editor = "nvim"

            [[accounts]]
            name    = "almnck"
            email   = "olddanny@almnck.com"
            maildir = "~/Private/almnck-mail"

            [accounts.smtp]
            host             = "smtp.purelymail.com"
            port             = 465
            encryption       = "ssl"
            username         = "olddanny@almnck.com"
            password_command = "pass email/work"

            [accounts.folders]
            inbox   = "/Inbox"
            archive = "/Archive"
            drafts  = "/Drafts"
            sent    = "/Sent"
            trash   = "/Trash"
            spam    = "/Junk"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.accounts.len(), 1, "accounts should have 1 entry");
        assert_eq!(cfg.accounts[0].maildir, "~/Private/almnck-mail");
        assert!(cfg.bindings.global.is_empty());
    }

    #[test]
    fn smtp_defaults() {
        let smtp = SmtpConfig::default();
        assert_eq!(smtp.host, "localhost");
        assert_eq!(smtp.port, 587);
        assert_eq!(smtp.encryption, "starttls");
        assert!(smtp.password.is_none());
        assert!(smtp.password_command.is_none());
        assert!(smtp.oauth2_command.is_none());
    }

    #[test]
    fn folder_defaults() {
        let folders = FolderConfig::default();
        assert_eq!(folders.inbox, "/Inbox");
        assert_eq!(folders.archive, "/Archive");
        assert_eq!(folders.drafts, "/Drafts");
        assert_eq!(folders.sent, "/Sent");
        assert_eq!(folders.trash, "/Trash");
        assert_eq!(folders.spam, "/Spam");
    }

    #[test]
    fn parse_multi_account_config() {
        let toml_str = r#"
            [[accounts]]
            name    = "Work"
            email   = "work@example.com"
            maildir = "~/Maildir/work"
            muhome  = "~/.cache/mu/work"
            default = true
            sync_command = "mbsync work"

            [accounts.smtp]
            host = "smtp.example.com"
            port = 465
            encryption = "ssl"
            username = "work@example.com"

            [[accounts]]
            name    = "Personal"
            email   = "me@personal.example"
            maildir = "~/Maildir/personal"

            [accounts.smtp]
            host = "smtp.personal.example"
            port = 587
            encryption = "starttls"
            username = "me@personal.example"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.accounts[0].muhome.as_deref(), Some("~/.cache/mu/work"));
        assert!(cfg.accounts[0].default);
        assert_eq!(cfg.accounts[0].sync_command.as_deref(), Some("mbsync work"));
        assert!(cfg.accounts[1].muhome.is_none());
        assert!(!cfg.accounts[1].default);
        assert!(cfg.accounts[1].sync_command.is_none());
    }

    #[test]
    fn default_account_index_first_default() {
        let toml_str = r#"
            [[accounts]]
            name = "A"
            email = "a@a.com"
            maildir = "~/a"
            [accounts.smtp]
            host = "smtp.a.com"

            [[accounts]]
            name = "B"
            email = "b@b.com"
            maildir = "~/b"
            default = true
            [accounts.smtp]
            host = "smtp.b.com"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.default_account_index(), 1);
    }

    #[test]
    fn default_account_index_no_default() {
        let toml_str = r#"
            [[accounts]]
            name = "A"
            email = "a@a.com"
            maildir = "~/a"
            [accounts.smtp]
            host = "smtp.a.com"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.default_account_index(), 0);
    }

    #[test]
    fn effective_sync_command_account_overrides_global() {
        let toml_str = r#"
            sync_command = "mbsync -a"

            [[accounts]]
            name = "Work"
            email = "w@w.com"
            maildir = "~/w"
            sync_command = "mbsync work"
            [accounts.smtp]
            host = "smtp.w.com"

            [[accounts]]
            name = "Personal"
            email = "p@p.com"
            maildir = "~/p"
            [accounts.smtp]
            host = "smtp.p.com"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.effective_sync_command(0), Some("mbsync work"));
        assert_eq!(cfg.effective_sync_command(1), Some("mbsync -a"));
    }

    #[test]
    fn effective_muhome_auto_derive() {
        let toml_str = r#"
            [[accounts]]
            name = "Work"
            email = "w@w.com"
            maildir = "~/w"
            [accounts.smtp]
            host = "smtp.w.com"

            [[accounts]]
            name = "Personal"
            email = "p@p.com"
            maildir = "~/p"
            [accounts.smtp]
            host = "smtp.p.com"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // Multi-account with no explicit muhome → auto-derived
        let muhome = cfg.effective_muhome(0).unwrap();
        assert!(muhome.ends_with("/.cache/mu/Work"));
        let muhome = cfg.effective_muhome(1).unwrap();
        assert!(muhome.ends_with("/.cache/mu/Personal"));
    }

    #[test]
    fn effective_muhome_single_account_none() {
        let toml_str = r#"
            [[accounts]]
            name = "Only"
            email = "o@o.com"
            maildir = "~/o"
            [accounts.smtp]
            host = "smtp.o.com"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // Single account, no explicit muhome → None (use system default)
        assert!(cfg.effective_muhome(0).is_none());
    }
}
