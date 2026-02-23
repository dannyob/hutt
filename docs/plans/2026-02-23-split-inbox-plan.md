# Split Inbox Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Partition the inbox into named categories via mu queries, excluding matched messages from the main inbox view.

**Architecture:** New `splits.rs` module (parallel to `smart_folders.rs`) handles persistence. `App` gains split caches (`Vec<HashSet<u32>>`) populated eagerly on account load and after reindex. Inbox filtering is client-side in `load_folder()`. Splits appear as `#name` folders in the picker. Creation reuses the smart folder two-phase UI flow via a `creating_split` flag.

**Tech Stack:** Rust, serde/toml for persistence, mu queries via existing `MuClient::find()`

---

### Task 1: Split persistence module

Create `src/splits.rs` — parallel to `src/smart_folders.rs`. Handles load/save of per-account split definitions.

**Files:**
- Create: `src/splits.rs`
- Modify: `src/main.rs` (add `mod splits;`)

**Step 1: Write the test**

Add to the bottom of `src/splits.rs`:

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test load_save_roundtrip -- splits`
Expected: compile error — `splits` module doesn't exist yet

**Step 3: Write the implementation**

Create `src/splits.rs`:

```rust
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
```

Add to `src/main.rs` after `mod smart_folders;`:

```rust
mod splits;
```

**Step 4: Run test to verify it passes**

Run: `cargo test splits::tests::load_save_roundtrip -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/splits.rs src/main.rs
git commit -m "feat(splits): add persistence module"
```

---

### Task 2: App state for splits and caches

Add split state to `App`: the split list, query map, and docid caches. Wire up loading on startup and account switch.

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Add imports and fields**

At `src/tui/mod.rs:38` after the `smart_folders` import, add:

```rust
use crate::splits::{self, Split};
```

Add fields to `App` struct (after line ~119, after `smart_folder_queries`):

```rust
    pub splits: Vec<Split>,
    pub split_queries: HashMap<String, String>,   // "#name" -> query
    pub split_excluded: HashSet<u32>,              // union of all split caches
    pub creating_split: bool,                      // true = create-flow saves as split
```

**Step 2: Initialize in `App::new()`**

After the smart folder initialization block (~lines 213-227), add parallel split loading:

```rust
        let splits = splits::load_splits(acct_name);
        let split_queries: HashMap<String, String> = splits
            .iter()
            .map(|s| (format!("#{}", s.name), s.query.clone()))
            .collect();
        for s in &splits {
            known_folders.push(format!("#{}", s.name));
        }
```

And in the struct literal (~line 258), add:

```rust
            splits,
            split_queries,
            split_excluded: HashSet::new(),
            creating_split: false,
```

**Step 3: Wire up account switch**

In `switch_account()` (~line 684 area, after smart folder reload), add parallel split reload:

```rust
        self.splits = splits::load_splits(&acct_name);
        self.split_queries = self.splits
            .iter()
            .map(|s| (format!("#{}", s.name), s.query.clone()))
            .collect();
        self.split_excluded.clear();
        for s in &self.splits {
            self.known_folders.push(format!("#{}", s.name));
        }
```

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: success (unused field warnings are fine for now)

**Step 5: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(splits): add App state for splits and caches"
```

---

### Task 3: Split cache refresh and inbox filtering

Implement `refresh_split_caches()` and integrate it into `load_folder()` and the reindex-complete path.

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Add `refresh_split_caches()` method**

Add to the `impl App` block, near `load_folder()` (~line 283):

```rust
    /// Run each split query against the inbox and cache the resulting docids.
    /// Builds the combined `split_excluded` set used to filter the inbox view.
    async fn refresh_split_caches(&mut self) {
        self.split_excluded.clear();
        let inbox_folder = self.account()
            .map(|a| a.folders.inbox.clone())
            .unwrap_or_else(|| "/Inbox".to_string());
        for split in &self.splits {
            let query = format!("maildir:{} AND ({})", inbox_folder, split.query);
            match self.mu.find(&query, &FindOpts { max_num: 10000, threads: false, ..Default::default() }).await {
                Ok(envelopes) => {
                    for e in &envelopes {
                        self.split_excluded.insert(e.docid);
                    }
                }
                Err(e) => {
                    debug_log!("split cache error for {:?}: {}", split.name, e);
                }
            }
        }
    }
```

Note: `threads: false` and `max_num: 10000` — we want raw docids, not threaded results, and a generous limit. No sorting needed.

**Step 2: Integrate into `load_folder()`**

In `load_folder()` (~line 283), after `self.envelopes = self.mu.find(...)`:

```rust
        // If viewing the inbox, exclude messages that belong to splits
        if self.is_inbox_folder() {
            self.envelopes.retain(|e| !self.split_excluded.contains(&e.docid));
        }
```

Add the helper:

```rust
    /// Check if the current folder is the account's inbox.
    fn is_inbox_folder(&self) -> bool {
        let inbox = self.account()
            .map(|a| a.folders.inbox.as_str())
            .unwrap_or("/Inbox");
        self.current_folder == inbox
    }
```

**Step 3: Add `build_query()` branch for splits**

In `build_query()` (~line 297), add a branch for `#` prefix after the `@` smart folder branch:

```rust
        let mut query = if let Some(q) = self.smart_folder_queries.get(&self.current_folder) {
            q.clone()
        } else if let Some(q) = self.split_queries.get(&self.current_folder) {
            let inbox_folder = self.account()
                .map(|a| a.folders.inbox.clone())
                .unwrap_or_else(|| "/Inbox".to_string());
            format!("maildir:{} AND ({})", inbox_folder, q)
        } else if self.current_folder.starts_with('/') {
            format!("maildir:{}", self.current_folder)
        } else {
            self.current_folder.clone()
        };
```

**Step 4: Call refresh on startup**

In `run()`, after the initial `app.load_folder().await?` (~line in the run function, after first load_folder call), add:

```rust
    app.refresh_split_caches().await;
    // Reload inbox if that's where we started, now with exclusions applied
    if app.is_inbox_folder() {
        app.load_folder().await?;
    }
```

**Step 5: Call refresh after reindex**

In the `poll_index_frame` handler in `run()` (~line 2177 area), after `app.load_folder()`:

```rust
                        app.refresh_split_caches().await;
                        // If we just reloaded a non-inbox folder, the inbox will
                        // pick up the new exclusions next time it's loaded.
                        // If we ARE on the inbox, reload again with fresh exclusions.
                        if app.is_inbox_folder() {
                            if let Err(e) = app.load_folder().await {
                                debug_log!("reindex: split-filtered reload error: {}", e);
                            }
                        }
```

**Step 6: Call refresh after account switch**

In `switch_account()`, after the existing `self.load_folder().await?`:

```rust
        self.refresh_split_caches().await;
        if self.is_inbox_folder() {
            self.load_folder().await?;
        }
```

**Step 7: Verify it compiles and tests pass**

Run: `cargo test`
Expected: all tests pass

**Step 8: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(splits): inbox filtering via cached split docids"
```

---

### Task 4: UndoAction for split deletion

Add `DeleteSplit` variant to `UndoAction` and handle it in the undo handler.

**Files:**
- Modify: `src/undo.rs`
- Modify: `src/tui/mod.rs`

**Step 1: Add variant to `UndoAction`**

In `src/undo.rs`, add after `DeleteSmartFolder`:

```rust
    DeleteSplit {
        split: Split,
    },
```

Add import at top of `src/undo.rs`:

```rust
use crate::splits::Split;
```

**Step 2: Handle undo in App**

In `src/tui/mod.rs`, in the `Action::Undo` handler (~line 609 area), after the `DeleteSmartFolder` arm, add:

```rust
                UndoAction::DeleteSplit { split } => {
                    self.splits.push(split.clone());
                    splits::save_splits(&self.splits, self.account_name());
                    let key = format!("#{}", split.name);
                    self.split_queries.insert(key.clone(), split.query);
                    self.known_folders.push(key);
                    self.known_folders.sort();
                    self.refresh_split_caches().await;
                    if self.is_inbox_folder() {
                        self.load_folder().await?;
                    }
                }
```

**Step 3: Verify it compiles**

Run: `cargo check`

**Step 4: Commit**

```bash
git add src/undo.rs src/tui/mod.rs
git commit -m "feat(splits): add DeleteSplit undo action"
```

---

### Task 5: Split deletion from folder picker

When viewing a `#split` in the folder picker, allow deleting it (same pattern as `@` smart folders).

**Files:**
- Modify: `src/tui/mod.rs` — `delete_selected_folder()`

**Step 1: Add `#` branch to `delete_selected_folder()`**

In `delete_selected_folder()` (~line 787), after the `@` smart folder deletion block and before the `/` maildir block, add:

```rust
        } else if let Some(name) = folder.strip_prefix('#') {
            // Split — remove from list and save
            if let Some(pos) = self.splits.iter().position(|s| s.name == name) {
                let removed = self.splits.remove(pos);
                splits::save_splits(&self.splits, self.account_name());
                self.split_queries.remove(&folder);
                self.known_folders.retain(|f| f != &folder);
                self.refresh_split_caches().await;
                self.undo_stack.push(UndoEntry {
                    action: UndoAction::DeleteSplit { split: removed },
                    description: format!("Deleted split {}", name),
                });
                self.set_status(format!("Deleted split \"{}\" (z to undo)", name));
                let max = self.filtered_folders().len();
                if self.folder_selected >= max && max > 0 {
                    self.folder_selected = max - 1;
                }
            }
```

Note: `delete_selected_folder` needs to become `async` since `refresh_split_caches` is async. Check that call sites use `.await`.

**Step 2: Verify it compiles**

Run: `cargo check`

**Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(splits): delete split from folder picker with undo"
```

---

### Task 6: Split creation flow

Reuse the smart folder create UI. When `creating_split` is true, the submit handler saves to splits instead of smart folders.

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/folder_picker.rs` (popup title)

**Step 1: Add "New split" entry to folder picker**

In `filtered_folders()` (~line 971), add after the "New maildir folder" entry:

```rust
        result.push("+ New split".to_string());
```

In `delete_selected_folder()`, add `"+ New split"` to the special entry check:

The existing check `if folder.starts_with("+ ")` already covers this.

**Step 2: Handle the new entry in `InputSubmit` for `FolderPicker`**

In the `InputSubmit` / `FolderPicker` handler (~line 1509), add a branch after the "New maildir folder" branch:

```rust
                        } else if folder == "+ New split" {
                            self.smart_create_query.clear();
                            self.smart_create_name.clear();
                            self.smart_create_phase = 0;
                            self.smart_create_preview.clear();
                            self.smart_create_count = None;
                            self.creating_split = true;
                            self.mode = InputMode::SmartFolderCreate;
```

And update the existing "New smart folder" branch to explicitly set `creating_split = false`:

```rust
                        if folder == "+ New smart folder" {
                            self.smart_create_query.clear();
                            self.smart_create_name.clear();
                            self.smart_create_phase = 0;
                            self.smart_create_preview.clear();
                            self.smart_create_count = None;
                            self.creating_split = false;
                            self.mode = InputMode::SmartFolderCreate;
```

**Step 3: Handle `InputSubmit` for `SmartFolderName` when creating a split**

In the `SmartFolderName` submit handler (~line 1540), wrap the existing smart folder save in an if/else:

```rust
                InputMode::SmartFolderName => {
                    let name = self.smart_create_name.trim().to_string();
                    let query = self.smart_create_query.trim().to_string();
                    if !name.is_empty() && !query.is_empty() {
                        if self.creating_split {
                            let split = Split {
                                name: name.clone(),
                                query: query.clone(),
                            };
                            self.splits.push(split);
                            splits::save_splits(&self.splits, self.account_name());
                            let key = format!("#{}", name);
                            self.split_queries.insert(key.clone(), query);
                            self.known_folders.push(key.clone());
                            self.known_folders.sort();
                            self.refresh_split_caches().await;
                            self.mode = InputMode::Normal;
                            self.navigate_folder(&key).await?;
                        } else {
                            let sf = SmartFolder {
                                name: name.clone(),
                                query: query.clone(),
                            };
                            self.smart_folders.push(sf);
                            smart_folders::save_smart_folders(&self.smart_folders, self.account_name());
                            let key = format!("@{}", name);
                            self.smart_folder_queries.insert(key.clone(), query);
                            self.known_folders.push(key.clone());
                            self.known_folders.sort();
                            self.mode = InputMode::Normal;
                            self.navigate_folder(&key).await?;
                        }
                        self.creating_split = false;
                    }
                }
```

**Step 4: Update popup title**

In `src/tui/folder_picker.rs`, the `SmartFolderPopup` widget has a hardcoded title. We need to pass through whether this is a split. The simplest approach: add a `title: &'a str` field to `SmartFolderPopup`.

In `src/tui/folder_picker.rs`, find the `SmartFolderPopup` struct and add:

```rust
    pub title: &'a str,
```

Update its render method to use `self.title` instead of the hardcoded `" New smart folder "` string.

In `src/tui/mod.rs` where `SmartFolderPopup` is rendered (~line 1970), pass the title:

```rust
                let popup = folder_picker::SmartFolderPopup {
                    query: &app.smart_create_query,
                    name: &app.smart_create_name,
                    phase: app.smart_create_phase,
                    preview: &app.smart_create_preview,
                    count: app.smart_create_count,
                    title: if app.creating_split { " New split " } else { " New smart folder " },
                };
```

**Step 5: Reset `creating_split` on Escape**

In the `InputCancel` handler for `SmartFolderCreate` and `SmartFolderName` (~line 1611), add:

```rust
                InputMode::SmartFolderCreate => {
                    self.creating_split = false;
                    self.mode = InputMode::FolderPicker;
                }
```

And for `SmartFolderName` going back to create phase (~line 1617):
No change needed — if they go back to query phase, `creating_split` stays set.
On full cancel (Escape from SmartFolderName), also reset:

Check the existing Escape handler for SmartFolderName. If it goes back to SmartFolderCreate, that's fine. If it cancels entirely, add `self.creating_split = false;`.

**Step 6: Verify it compiles and tests pass**

Run: `cargo test`

**Step 7: Commit**

```bash
git add src/tui/mod.rs src/tui/folder_picker.rs
git commit -m "feat(splits): create split via folder picker UI"
```

---

### Task 7: Exclude splits from move targets

Splits shouldn't appear as move-to-folder destinations.

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Filter `#` from `filtered_folders_plain()`**

In `filtered_folders_plain()` (~line 996), add a filter condition:

```rust
    fn filtered_folders_plain(&self) -> Vec<String> {
        let filter = self.folder_filter.to_lowercase();
        self.known_folders
            .iter()
            .filter(|f| {
                // Exclude splits from move targets
                if f.starts_with('#') {
                    return false;
                }
                if filter.is_empty() {
                    return true;
                }
                f.to_lowercase().contains(&filter)
                    || f.strip_prefix('@')
                        .is_some_and(|name| name.to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
    }
```

**Step 2: Verify it compiles**

Run: `cargo check`

**Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(splits): exclude splits from move-to-folder targets"
```

---

### Task 8: Command palette entry

Add "Create Split" to the command palette.

**Files:**
- Modify: `src/keymap.rs` — add `CreateSplit` action
- Modify: `src/tui/command_palette.rs` — add palette entry
- Modify: `src/tui/mod.rs` — handle `CreateSplit` action

**Step 1: Add Action variant**

In `src/keymap.rs`, in the `Action` enum, add:

```rust
    CreateSplit,
```

In the `action_from_name` function, add:

```rust
        "create_split" => Ok(Action::CreateSplit),
```

In `ACTION_NAMES`, add:

```rust
            "create_split",
```

**Step 2: Add palette entry**

In `src/tui/command_palette.rs`, in `all_actions()`, add before the Help entry:

```rust
            PaletteEntry {
                name: "Create Split".into(),
                description: "Create an inbox split (partitions inbox by query)".into(),
                shortcut: None,
                action: Action::CreateSplit,
            },
```

**Step 3: Handle the action**

In `src/tui/mod.rs`, in `handle_action()`, add a handler:

```rust
            Action::CreateSplit => {
                self.smart_create_query.clear();
                self.smart_create_name.clear();
                self.smart_create_phase = 0;
                self.smart_create_preview.clear();
                self.smart_create_count = None;
                self.creating_split = true;
                self.mode = InputMode::SmartFolderCreate;
            }
```

**Step 4: Verify it compiles and tests pass**

Run: `cargo test`

**Step 5: Commit**

```bash
git add src/keymap.rs src/tui/command_palette.rs src/tui/mod.rs
git commit -m "feat(splits): add Create Split to command palette"
```

---

### Task 9: Filtered folders — split matching

Ensure `#` splits are filterable in the folder picker the same way `@` smart folders are.

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Update `filtered_folders()`**

In `filtered_folders()`, update the matching logic to also match `#` prefixed splits:

```rust
                let matches = f.to_lowercase().contains(&filter)
                    || f.strip_prefix('@')
                        .is_some_and(|name| name.to_lowercase().contains(&filter))
                    || f.strip_prefix('#')
                        .is_some_and(|name| name.to_lowercase().contains(&filter));
```

**Step 2: Verify it compiles**

Run: `cargo check`

**Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(splits): enable split filtering in folder picker"
```

---

### Task 10: Final integration test and cleanup

Run the full test suite, clippy, and do a release build.

**Step 1: Run tests**

Run: `cargo test`
Expected: all tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -W clippy::all`
Expected: no errors (warnings about unused fields should be gone now)

**Step 3: Build release**

Run: `make build`
Expected: success

**Step 4: Install**

Run: `make install`

**Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "feat(splits): split inbox complete"
```
