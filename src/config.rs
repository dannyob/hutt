use anyhow::{Context, Result};
use serde::Deserialize;
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
    pub keybindings: KeybindingOverrides,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            editor: "nvim".to_string(),
            sync_command: None,
            snippets: Vec::new(),
            keybindings: KeybindingOverrides::default(),
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
// Keybinding overrides (placeholder for future use)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct KeybindingOverrides {
    /// Map of action name -> key string, e.g. { "archive" = "A" }.
    /// Interpretation is deferred until the keymap module consumes this.
    #[serde(flatten)]
    pub overrides: std::collections::HashMap<String, String>,
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
    fn parse_keybinding_overrides() {
        let toml_str = r#"
            [keybindings]
            archive = "A"
            delete = "D"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.keybindings.overrides.get("archive").unwrap(), "A");
        assert_eq!(cfg.keybindings.overrides.get("delete").unwrap(), "D");
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
