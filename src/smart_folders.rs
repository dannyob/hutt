use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFolder {
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SmartFoldersFile {
    #[serde(default)]
    folders: Vec<SmartFolder>,
}

/// Return the path to `smart_folders.toml`, using the same XDG logic as config.rs.
pub fn smart_folders_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("hutt").join("smart_folders.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("hutt")
            .join("smart_folders.toml")
    } else {
        PathBuf::from("smart_folders.toml")
    }
}

/// Load smart folders from disk. Returns empty vec if file is missing or invalid.
pub fn load_smart_folders() -> Vec<SmartFolder> {
    let path = smart_folders_path();
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let file: SmartFoldersFile = match toml::from_str(&contents) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    file.folders
}

/// Save smart folders to disk. Creates parent directories if needed.
pub fn save_smart_folders(folders: &[SmartFolder]) {
    let path = smart_folders_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = SmartFoldersFile {
        folders: folders.to_vec(),
    };
    if let Ok(contents) = toml::to_string_pretty(&file) {
        let _ = std::fs::write(&path, contents);
    }
}

/// Known mu field prefixes for search throttling.
const FIELD_PREFIXES: &[&str] = &[
    "from:", "to:", "cc:", "bcc:", "subject:", "body:", "date:", "flag:", "prio:",
    "mime:", "maildir:", "tag:", "list:", "msgid:", "embed:", "file:",
];

/// Determine whether a query string is "ready" to search — used to throttle
/// live preview during smart folder creation.
///
/// Rules:
/// 1. Total query length must be >= 3 characters.
/// 2. The last space-separated token (the "active term") must have >= 3
///    characters after any recognized field prefix.
pub fn should_search(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let active_term = trimmed.split_whitespace().last().unwrap_or("");
    let value_part = strip_field_prefix(active_term);
    value_part.len() >= 3
}

/// Strip a recognized mu field prefix (e.g. `from:`) from a term.
fn strip_field_prefix(term: &str) -> &str {
    let lower = term.to_lowercase();
    for prefix in FIELD_PREFIXES {
        if lower.starts_with(prefix) {
            return &term[prefix.len()..];
        }
    }
    term
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_search_short_query() {
        assert!(!should_search(""));
        assert!(!should_search("ab"));
        assert!(should_search("abc"));
    }

    #[test]
    fn should_search_field_prefix() {
        assert!(!should_search("from:da"));
        assert!(should_search("from:dan"));
        assert!(should_search("from:danny"));
    }

    #[test]
    fn should_search_multi_term() {
        // First term is fine, but active term (last) is too short
        assert!(!should_search("from:danny to:da"));
        assert!(should_search("from:danny to:dan"));
    }

    #[test]
    fn should_search_no_prefix() {
        assert!(should_search("hello world"));
        assert!(!should_search("hello ab"));
    }

    #[test]
    fn should_search_unknown_prefix_treated_as_value() {
        // "foobar:" is not a recognized prefix, so the whole thing is the value
        assert!(should_search("foobar:x"));
    }

    #[test]
    fn load_save_roundtrip() {
        let dir = std::env::temp_dir().join("hutt-test-smart-folders");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smart_folders.toml");

        let folders = vec![
            SmartFolder {
                name: "Unread from Danny".into(),
                query: "from:danny flag:unread".into(),
            },
            SmartFolder {
                name: "Recent attachments".into(),
                query: "mime:application/* date:1w..".into(),
            },
        ];

        // Write manually for the test
        let file = SmartFoldersFile {
            folders: folders.clone(),
        };
        let contents = toml::to_string_pretty(&file).unwrap();
        std::fs::write(&path, &contents).unwrap();

        // Read back and verify
        let parsed: SmartFoldersFile = toml::from_str(&contents).unwrap();
        assert_eq!(parsed.folders.len(), 2);
        assert_eq!(parsed.folders[0].name, "Unread from Danny");
        assert_eq!(parsed.folders[0].query, "from:danny flag:unread");
        assert_eq!(parsed.folders[1].name, "Recent attachments");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
