use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Split {
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SplitsFile {
    #[serde(default)]
    splits: Vec<Split>,
}

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("hutt")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("hutt")
    } else {
        PathBuf::from(".")
    }
}

pub fn splits_path(account_name: &str) -> PathBuf {
    let dir = config_dir();
    if account_name.is_empty() {
        dir.join("splits.toml")
    } else {
        dir.join(format!("splits.{}.toml", account_name))
    }
}

/// Load splits for an account. Falls back to the plain
/// `splits.toml` if no per-account file exists (migration path).
pub fn load_splits(account_name: &str) -> Vec<Split> {
    let path = splits_path(account_name);
    if let Ok(contents) = std::fs::read_to_string(&path) {
        if let Ok(file) = toml::from_str::<SplitsFile>(&contents) {
            return file.splits;
        }
    }
    if !account_name.is_empty() {
        let fallback = splits_path("");
        if let Ok(contents) = std::fs::read_to_string(&fallback) {
            if let Ok(file) = toml::from_str::<SplitsFile>(&contents) {
                return file.splits;
            }
        }
    }
    Vec::new()
}

/// Save splits for an account. Creates parent directories if needed.
pub fn save_splits(splits: &[Split], account_name: &str) {
    let path = splits_path(account_name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = SplitsFile {
        splits: splits.to_vec(),
    };
    if let Ok(contents) = toml::to_string_pretty(&file) {
        let _ = std::fs::write(&path, contents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_save_roundtrip() {
        let dir = std::env::temp_dir().join("hutt-test-splits");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("splits.test-acct.toml");

        let splits = vec![
            Split { name: "Newsletters".into(), query: "list:*".into() },
            Split { name: "GitHub".into(), query: "from:notifications@github.com".into() },
        ];

        let file = SplitsFile { splits: splits.clone() };
        let contents = toml::to_string_pretty(&file).unwrap();
        std::fs::write(&path, &contents).unwrap();

        let parsed: SplitsFile = toml::from_str(&contents).unwrap();
        assert_eq!(parsed.splits.len(), 2);
        assert_eq!(parsed.splits[0].name, "Newsletters");
        assert_eq!(parsed.splits[0].query, "list:*");
        assert_eq!(parsed.splits[1].name, "GitHub");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
