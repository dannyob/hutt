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
pub struct AccountConfig {
    pub name: String,
    pub email: String,
    pub maildir: String,
    pub smtp: SmtpConfig,
    #[serde(default)]
    pub folders: FolderConfig,
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
/// Future expansion: `{ action = "move", folder = "/Archive/2026" }` for
/// parameterized actions (not yet implemented).
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
}
