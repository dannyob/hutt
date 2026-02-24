pub mod command_palette;
pub mod envelope_list;
pub mod folder_picker;
pub mod help_overlay;
pub mod preview;
pub mod status_bar;
pub mod thread_view;

use std::collections::HashSet;
use std::sync::OnceLock;

use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyEventKind},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::io;
use std::time::Duration;
use tokio::time::Instant;

use std::collections::HashMap;

use crate::compose;
use crate::config::Config;
use crate::envelope::{flags_from_string, group_into_conversations, Conversation, Envelope};
use crate::keymap::{Action, InputMode, KeyMapper};
use crate::links::{self, HuttUrl, IpcCommand, IpcListener};
use crate::mime_render::{self, RenderCache};
use crate::mu_client::{FindOpts, MuClient};
use crate::send;
use crate::smart_folders::{self, SmartFolder};
use crate::splits::{self, Split};
use crate::undo::{UndoAction, UndoEntry, UndoStack};

use self::command_palette::{CommandPalette, PaletteEntry};
use self::envelope_list::{ConversationList, EnvelopeList};
use self::folder_picker::FolderPicker;
use self::help_overlay::HelpOverlay;
use self::preview::PreviewPane;
use self::status_bar::{BottomBar, TopBar};
use self::thread_view::{ThreadMessage, ThreadView};

/// Write a debug line to the file at $HUTT_LOG (if set).
/// Usage: `debug_log!("IPC received: {:?}", cmd);`
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Some(path) = debug_log_path() {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", format_args!($($arg)*));
            }
        }
    };
}

fn debug_log_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("HUTT_LOG").ok())
        .as_deref()
}

// ── Tab bar types ───────────────────────────────────────────────────

pub struct PrefetchItem {
    pub account_idx: usize,
    pub folder: String,
    pub query: String,
}

pub struct TabRegion {
    pub x_start: u16,
    pub x_end: u16,
    pub kind: TabRegionKind,
}

pub enum TabRegionKind {
    Account,
    Tab(usize),   // index into app.tabs
    Overflow,     // the "…" button
}

/// Resolve the tab list from config + runtime folder data.
///
/// `config_tabs` is the user's `tabs` list (or None for default).
/// Wildcards: `"/"` = remaining maildir folders, `"#"` = remaining splits,
/// `"@"` = remaining smart folders.
fn resolve_tabs(
    config_tabs: Option<&[String]>,
    account_folders: &[String],  // named folders: inbox, archive, drafts, sent, trash, spam
    splits: &[String],           // "#name" entries
    smart_folders: &[String],    // "@name" entries
) -> Vec<String> {
    let default_tabs: Vec<String> = vec![
        account_folders.first().cloned().unwrap_or_else(|| "/Inbox".to_string()),
        "#".to_string(),
        "/".to_string(),
        "@".to_string(),
    ];
    let config = config_tabs.unwrap_or(&default_tabs);

    let mut result: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for entry in config {
        match entry.as_str() {
            "#" => {
                for s in splits {
                    if seen.insert(s.clone()) {
                        result.push(s.clone());
                    }
                }
            }
            "@" => {
                for s in smart_folders {
                    if seen.insert(s.clone()) {
                        result.push(s.clone());
                    }
                }
            }
            "/" => {
                for f in account_folders {
                    if seen.insert(f.clone()) {
                        result.push(f.clone());
                    }
                }
            }
            _ => {
                if seen.insert(entry.clone()) {
                    result.push(entry.clone());
                }
            }
        }
    }

    result
}

pub struct App {
    // Active account (index into config.accounts)
    pub active_account: usize,

    // Core state
    pub current_folder: String,
    pub current_query: String,
    pub envelopes: Vec<Envelope>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub preview_scroll: u16,
    pub preview_cache: RenderCache,
    pub mu: MuClient,
    pub keymap: KeyMapper,
    pub should_quit: bool,

    // Mode
    pub mode: InputMode,

    // Undo
    pub undo_stack: UndoStack,

    // Multi-select
    pub selected_set: HashSet<u32>,

    // Search
    pub search_input: String,
    pub previous_folder: Option<String>,
    pub search_history: Vec<String>,
    pub search_history_index: Option<usize>,

    // Filters
    pub filter_unread: bool,
    pub filter_starred: bool,
    pub filter_needs_reply: bool,

    // Thread view
    pub thread_messages: Vec<ThreadMessage>,
    pub thread_selected: usize,
    pub thread_scroll: u16,

    // Folder picker
    pub known_folders: Vec<String>,
    pub folder_filter: String,
    pub folder_selected: usize,

    // Smart folders
    pub smart_folders: Vec<SmartFolder>,
    pub smart_folder_queries: HashMap<String, String>, // "@name" -> query

    // Splits (inbox partitions)
    pub splits: Vec<Split>,
    pub split_queries: HashMap<String, String>,   // "#name" -> query
    pub split_excluded: HashSet<u32>,              // union of all split caches
    pub creating_split: bool,                      // true = create-flow saves as split

    // Smart folder creation
    pub smart_create_query: String,
    pub smart_create_name: String,
    pub smart_create_phase: u8, // 0 = query, 1 = name
    pub smart_create_preview: Vec<String>, // subject lines
    pub smart_create_count: Option<u32>,

    // Maildir creation
    pub maildir_create_input: String,

    // Command palette
    pub palette_filter: String,
    pub palette_selected: usize,
    pub palette_entries: Vec<PaletteEntry>,

    // Conversations (grouped threads) mode
    pub conversations_mode: bool,
    pub conversations: Vec<Conversation>,

    // Tab bar
    pub tabs: Vec<String>,
    pub tab_scroll: usize,
    pub tab_regions: Vec<TabRegion>,
    pub account_picker_selected: usize,

    // Folder query cache: (account_index, query_string) → envelopes.
    // Invalidated per-account on triage/reindex. Survives account switches
    // so prefetched results for other accounts are ready immediately.
    pub folder_cache: HashMap<(usize, String), Vec<Envelope>>,
    // When true, collect_known_folders() will rescan the maildir tree.
    // Set on reindex and account switch; cleared after scan.
    pub known_folders_dirty: bool,
    // Queue of prefetch items to run during idle time.
    pub prefetch_queue: Vec<PrefetchItem>,
    // Background mu servers for non-active accounts (read-only prefetch).
    // On account switch, we swap rather than quit/restart.
    pub background_mu: HashMap<usize, MuClient>,

    // List/preview split (percentage for list pane, 10..90)
    pub list_pct: u16,
    pub dragging_border: bool,

    // Help overlay
    pub help_scroll: u16,

    // Status message (temporary feedback)
    pub status_message: Option<String>,
    pub status_time: Option<Instant>,

    // Compose pending (set by action handler, processed by run loop)
    pub compose_pending: Option<compose::ComposePending>,

    // Shell command pending (suspend=true, processed by run loop like compose)
    pub shell_pending: Option<ShellPending>,

    // Set when a background shell command finishes with reindex=true
    pub needs_reindex: bool,

    // True while mu server is processing an (index) command
    pub indexing: bool,

    // Channel sender for background shell command results (receiver lives in run loop)
    shell_tx: tokio::sync::mpsc::UnboundedSender<Result<ShellResult, ShellError>>,

    // Config
    pub config: Config,
}

pub struct ShellPending {
    pub command: String,
    pub reindex: bool,
}

/// Result of a background (async) shell command.
struct ShellResult {
    command: String,
    reindex: bool,
    stdout: String,
    stderr: String,
    status: std::process::ExitStatus,
}

/// Error from a background shell command.
struct ShellError {
    command: String,
    error: String,
}

impl App {
    /// Return the active account config.
    pub fn account(&self) -> Option<&crate::config::AccountConfig> {
        self.config.accounts.get(self.active_account)
    }

    /// Return the active account's name (for per-account file paths).
    fn account_name(&self) -> &str {
        self.account().map(|a| a.name.as_str()).unwrap_or("")
    }

    /// Build the mu query string for a given folder name (without filters).
    /// Used for prefetch — we prefetch the base query without filter flags.
    fn query_for_folder(&self, folder: &str) -> String {
        if let Some(q) = self.smart_folder_queries.get(folder) {
            q.clone()
        } else if let Some(q) = self.split_queries.get(folder) {
            let inbox_folder = self.account()
                .map(|a| a.folders.inbox.clone())
                .unwrap_or_else(|| "/Inbox".to_string());
            format!("maildir:{} AND ({})", inbox_folder, q)
        } else if folder.starts_with('/') {
            format!("maildir:{}", folder)
        } else {
            folder.to_string()
        }
    }

    /// Build an ordered tab list for an account: neighbors of `center` expanding outward.
    /// If `center` is None (other account), starts with inbox then expands.
    fn tabs_for_account(&self, account_idx: usize, center: Option<&str>) -> Vec<String> {
        let account = match self.config.accounts.get(account_idx) {
            Some(a) => a,
            None => return Vec::new(),
        };
        let folder_list: Vec<String> = vec![
            account.folders.inbox.clone(),
            account.folders.archive.clone(),
            account.folders.drafts.clone(),
            account.folders.sent.clone(),
            account.folders.trash.clone(),
            account.folders.spam.clone(),
        ];
        // For non-active accounts we don't have live splits/smart folders,
        // so just use config-defined tabs with empty wildcard expansions.
        let (split_names, smart_names) = if account_idx == self.active_account {
            (
                self.splits.iter().map(|s| format!("#{}", s.name)).collect::<Vec<_>>(),
                self.smart_folders.iter().map(|sf| format!("@{}", sf.name)).collect::<Vec<_>>(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let tabs = resolve_tabs(
            account.tabs.as_deref(),
            &folder_list,
            &split_names,
            &smart_names,
        );
        if tabs.is_empty() {
            return Vec::new();
        }

        let mut ordered = Vec::new();
        if let Some(center_folder) = center {
            if let Some(idx) = tabs.iter().position(|t| t == center_folder) {
                // Expanding outward from current position
                for delta in 1..tabs.len() {
                    let next = (idx + delta) % tabs.len();
                    ordered.push(tabs[next].clone());
                    let prev = (idx + tabs.len() - delta) % tabs.len();
                    if prev != next {
                        ordered.push(tabs[prev].clone());
                    }
                }
            } else {
                ordered = tabs;
            }
        } else {
            // Other account: inbox first, then remaining
            ordered = tabs;
        }
        ordered
    }

    /// Build a query string for a folder on a specific account.
    fn query_for_account_folder(&self, account_idx: usize, folder: &str) -> String {
        if account_idx == self.active_account {
            return self.query_for_folder(folder);
        }
        // For non-active accounts, build basic maildir queries
        if folder.starts_with('/') {
            format!("maildir:{}", folder)
        } else {
            folder.to_string()
        }
    }

    /// Queue prefetch for adjacent tabs, interleaved across all accounts.
    /// Active account gets every other slot; other accounts' inboxes appear early.
    fn queue_prefetch(&mut self) {
        self.prefetch_queue.clear();

        // Build per-account ordered folder lists
        let active = self.active_account;
        let active_folders = self.tabs_for_account(active, Some(&self.current_folder));
        let mut other_folders: Vec<(usize, Vec<String>)> = Vec::new();
        if self.config.background_servers {
            for idx in 0..self.config.accounts.len() {
                if idx != active {
                    let folders = self.tabs_for_account(idx, None);
                    if !folders.is_empty() {
                        other_folders.push((idx, folders));
                    }
                }
            }
        }

        // Interleave: active, other[0], active, other[1], active, other[0], ...
        let mut active_iter = active_folders.into_iter();
        let mut other_iters: Vec<(usize, std::vec::IntoIter<String>)> = other_folders
            .into_iter()
            .map(|(idx, v)| (idx, v.into_iter()))
            .collect();
        let mut other_round = 0;
        let mut done = false;
        while !done {
            done = true;
            // One from active account
            if let Some(folder) = active_iter.next() {
                let query = self.query_for_folder(&folder);
                if !self.folder_cache.contains_key(&(active, query.clone())) {
                    self.prefetch_queue.push(PrefetchItem {
                        account_idx: active,
                        folder,
                        query,
                    });
                }
                done = false;
            }
            // One from the next other account (round-robin)
            if !other_iters.is_empty() {
                let start = other_round % other_iters.len();
                // Try each other account starting from current round-robin position
                for i in 0..other_iters.len() {
                    let pos = (start + i) % other_iters.len();
                    let (acct_idx, ref mut iter) = other_iters[pos];
                    if let Some(folder) = iter.next() {
                        let query = self.query_for_account_folder(acct_idx, &folder);
                        if !self.folder_cache.contains_key(&(acct_idx, query.clone())) {
                            self.prefetch_queue.push(PrefetchItem {
                                account_idx: acct_idx,
                                folder,
                                query,
                            });
                        }
                        done = false;
                        other_round = pos + 1;
                        break;
                    }
                }
                // Clean up exhausted iterators
                other_iters.retain(|(_, iter)| iter.len() > 0);
            }
        }
        if !self.prefetch_queue.is_empty() {
            debug_log!("prefetch: queued {} items across {} accounts",
                self.prefetch_queue.len(), 1 + self.config.accounts.len() - 1);
        }
    }

    /// Invalidate the folder query cache. Called after triage, reindex,
    /// and account switch — any event that changes mu's data.
    fn invalidate_folder_cache(&mut self) {
        let acct = self.active_account;
        let before = self.folder_cache.len();
        self.folder_cache.retain(|(idx, _), _| *idx != acct);
        let removed = before - self.folder_cache.len();
        if removed > 0 {
            debug_log!("invalidate_folder_cache: cleared {} entries for account {}", removed, acct);
        }
        self.prefetch_queue.clear();
        self.known_folders_dirty = true;
    }

    /// Adjust tab_scroll to keep the selected folder visible in the tab bar.
    fn adjust_tab_scroll(&mut self) {
        if let Some(idx) = self.tabs.iter().position(|t| t == &self.current_folder) {
            if idx == 0 {
                // Inbox is pinned, no scroll needed
                return;
            }
            // Ensure selected tab is within visible window.
            // tab_scroll is the first non-Inbox tab index shown.
            // Keep at least 1 neighbor before the selected tab.
            let min_scroll = if idx > 1 { idx - 1 } else { 1 };
            if self.tab_scroll > min_scroll {
                self.tab_scroll = min_scroll;
            }
            // We can't know exactly how many tabs fit without terminal width,
            // but we can ensure the selected tab isn't before the scroll window.
            if idx < self.tab_scroll {
                self.tab_scroll = if idx > 1 { idx - 1 } else { 1 };
            }
        }
    }

    /// Rebuild the tab list from current account config + runtime folder data.
    fn rebuild_tabs(&mut self) {
        let account = self.account();
        let account_folder_list: Vec<String> = account
            .map(|a| vec![
                a.folders.inbox.clone(),
                a.folders.archive.clone(),
                a.folders.drafts.clone(),
                a.folders.sent.clone(),
                a.folders.trash.clone(),
                a.folders.spam.clone(),
            ])
            .unwrap_or_default();
        let split_names: Vec<String> = self.splits.iter().map(|s| format!("#{}", s.name)).collect();
        let smart_folder_names: Vec<String> = self.smart_folders.iter().map(|sf| format!("@{}", sf.name)).collect();
        self.tabs = resolve_tabs(
            account.and_then(|a| a.tabs.as_deref()),
            &account_folder_list,
            &split_names,
            &smart_folder_names,
        );
        self.tab_scroll = 0;
    }

    pub async fn new(mu: MuClient, config: Config) -> Result<Self> {
        debug_log!("App::new: accounts={} editor={:?} bindings_global={} bindings_normal={} bindings_thread={}",
            config.accounts.len(), config.editor,
            config.bindings.global.len(), config.bindings.normal.len(), config.bindings.thread.len());
        if let Some(acct) = config.accounts.first() {
            debug_log!("App::new: account[0] email={:?} maildir={:?}", acct.email, acct.maildir);
        }
        let mut keymap = KeyMapper::new();
        keymap.load_bindings(&config.bindings);

        let (shell_tx, _) = tokio::sync::mpsc::unbounded_channel();

        let active_account = config.default_account_index();

        // Load smart folders from disk for the default account
        let acct_name = config.accounts.get(active_account).map(|a| a.name.as_str()).unwrap_or("");
        let smart_folders = smart_folders::load_smart_folders(acct_name);
        let smart_folder_queries: HashMap<String, String> = smart_folders
            .iter()
            .map(|sf| (format!("@{}", sf.name), sf.query.clone()))
            .collect();
        let mut known_folders = vec![
            "/Inbox".into(),
            "/Archive".into(),
            "/Drafts".into(),
            "/Sent".into(),
            "/Trash".into(),
            "/Junk".into(),
        ];
        for sf in &smart_folders {
            known_folders.push(format!("@{}", sf.name));
        }

        // Load splits from disk for the default account
        let splits = splits::load_splits(acct_name);
        let split_queries: HashMap<String, String> = splits
            .iter()
            .map(|s| (format!("#{}", s.name), s.query.clone()))
            .collect();
        for s in &splits {
            known_folders.push(format!("#{}", s.name));
        }

        // Resolve tab bar
        let account = config.accounts.get(active_account);
        let account_folder_list: Vec<String> = account
            .map(|a| vec![
                a.folders.inbox.clone(),
                a.folders.archive.clone(),
                a.folders.drafts.clone(),
                a.folders.sent.clone(),
                a.folders.trash.clone(),
                a.folders.spam.clone(),
            ])
            .unwrap_or_default();
        let split_names: Vec<String> = splits.iter().map(|s| format!("#{}", s.name)).collect();
        let smart_folder_names: Vec<String> = smart_folders.iter().map(|sf| format!("@{}", sf.name)).collect();
        let tabs = resolve_tabs(
            account.and_then(|a| a.tabs.as_deref()),
            &account_folder_list,
            &split_names,
            &smart_folder_names,
        );

        Ok(Self {
            active_account,
            current_folder: "/Inbox".to_string(),
            current_query: String::new(),
            envelopes: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            preview_scroll: 0,
            preview_cache: RenderCache::new(),
            mu,
            keymap,
            should_quit: false,
            mode: InputMode::Normal,
            undo_stack: UndoStack::new(),
            selected_set: HashSet::new(),
            search_input: String::new(),
            previous_folder: None,
            search_history: Vec::new(),
            search_history_index: None,
            filter_unread: false,
            filter_starred: false,
            filter_needs_reply: false,
            thread_messages: Vec::new(),
            thread_selected: 0,
            thread_scroll: 0,
            known_folders,
            folder_filter: String::new(),
            folder_selected: 0,
            smart_folders,
            smart_folder_queries,
            splits,
            split_queries,
            split_excluded: HashSet::new(),
            creating_split: false,
            smart_create_query: String::new(),
            smart_create_name: String::new(),
            smart_create_phase: 0,
            smart_create_preview: Vec::new(),
            smart_create_count: None,
            maildir_create_input: String::new(),
            conversations_mode: config.conversations,
            conversations: Vec::new(),
            palette_filter: String::new(),
            palette_selected: 0,
            palette_entries: PaletteEntry::all_actions(),
            tabs,
            tab_scroll: 0,
            tab_regions: Vec::new(),
            account_picker_selected: 0,
            folder_cache: HashMap::new(),
            known_folders_dirty: true,
            prefetch_queue: Vec::new(),
            background_mu: HashMap::new(),
            list_pct: 35,
            dragging_border: false,
            help_scroll: 0,
            status_message: None,
            status_time: None,
            compose_pending: None,
            shell_pending: None,
            needs_reindex: false,
            indexing: false,
            shell_tx,
            config,
        })
    }

    pub async fn load_folder(&mut self) -> Result<()> {
        let query = self.build_query();
        debug_log!("load_folder: query={:?} folder={:?}", query, self.current_folder);
        self.current_query = query.clone();

        // Check folder cache before querying mu
        let cache_key = (self.active_account, query.clone());
        if let Some(cached) = self.folder_cache.get(&cache_key) {
            debug_log!("load_folder: cache HIT ({} envelopes)", cached.len());
            self.envelopes = cached.clone();
        } else {
            self.envelopes = self.mu.find(&query, &FindOpts::default()).await?;
            // If viewing the inbox, exclude messages that belong to splits
            if self.is_inbox_folder() && !self.split_excluded.is_empty() {
                let before = self.envelopes.len();
                self.envelopes.retain(|e| !self.split_excluded.contains(&e.docid));
                debug_log!("load_folder: split exclusion removed {} envelopes", before - self.envelopes.len());
            }
            debug_log!("load_folder: cache MISS, got {} envelopes from mu", self.envelopes.len());
            self.folder_cache.insert(cache_key, self.envelopes.clone());
        }

        self.selected = 0;
        self.scroll_offset = 0;
        self.preview_scroll = 0;
        self.rebuild_conversations();
        if self.known_folders_dirty {
            self.collect_known_folders();
            self.known_folders_dirty = false;
        }
        self.queue_prefetch();
        Ok(())
    }

    /// Check if the current folder is the account's inbox.
    fn is_inbox_folder(&self) -> bool {
        let inbox = self.account()
            .map(|a| a.folders.inbox.as_str())
            .unwrap_or("/Inbox");
        self.current_folder == inbox
    }

    /// Run each split query against the inbox and cache the resulting docids.
    /// Builds the combined `split_excluded` set used to filter the inbox view.
    async fn refresh_split_caches(&mut self) {
        self.split_excluded.clear();
        if self.splits.is_empty() {
            return;
        }
        let inbox_folder = self.account()
            .map(|a| a.folders.inbox.clone())
            .unwrap_or_else(|| "/Inbox".to_string());
        let opts = FindOpts {
            max_num: 10000,
            threads: false,
            descending: false,
            ..Default::default()
        };
        for split in &self.splits {
            let query = format!("maildir:{} AND ({})", inbox_folder, split.query);
            match self.mu.find(&query, &opts).await {
                Ok(envelopes) => {
                    debug_log!("split cache {:?}: {} docids", split.name, envelopes.len());
                    for e in &envelopes {
                        self.split_excluded.insert(e.docid);
                    }
                }
                Err(e) => {
                    debug_log!("split cache error for {:?}: {}", split.name, e);
                }
            }
        }
        debug_log!("split_excluded total: {} docids", self.split_excluded.len());

        // Invalidate cached inbox query since split exclusions changed.
        // Also invalidate cached split folder queries.
        let acct = self.active_account;
        let inbox_query = format!("maildir:{}", inbox_folder);
        self.folder_cache.remove(&(acct, inbox_query));
        for split in &self.splits {
            let q = format!("maildir:{} AND ({})", inbox_folder, split.query);
            self.folder_cache.remove(&(acct, q));
        }
    }

    fn build_query(&self) -> String {
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
        if self.filter_unread {
            query.push_str(" AND flag:unread");
        }
        if self.filter_starred {
            query.push_str(" AND flag:flagged");
        }
        if self.filter_needs_reply {
            query.push_str(" AND NOT flag:replied");
        }
        query
    }

    fn collect_known_folders(&mut self) {
        let mut folders: HashSet<String> = self.known_folders.iter().cloned().collect();
        for e in &self.envelopes {
            if !e.maildir.is_empty() {
                folders.insert(e.maildir.clone());
            }
        }
        // Scan maildir root recursively for all real folders
        if let Some(account) = self.account() {
            let root = expand_maildir_root(&account.maildir);
            let root_path = std::path::PathBuf::from(&root);
            let mut stack = vec![root_path.clone()];
            while let Some(dir) = stack.pop() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if path.join("cur").is_dir() {
                                if let Ok(rel) = path.strip_prefix(&root_path) {
                                    let name = rel.to_string_lossy();
                                    let name = name.strip_prefix('.').unwrap_or(&name);
                                    folders.insert(format!("/{}", name));
                                }
                                // Also recurse into it — there may be sub-maildirs
                                stack.push(path);
                            } else {
                                stack.push(path);
                            }
                        }
                    }
                }
            }
        }
        // Re-add smart folder entries so they persist across reloads
        for sf in &self.smart_folders {
            folders.insert(format!("@{}", sf.name));
        }
        self.known_folders = folders.into_iter().collect();
        self.known_folders.sort();
    }

    fn selected_envelope(&self) -> Option<&Envelope> {
        self.preview_envelope()
    }

    fn ensure_preview_loaded(&mut self, width: u16) {
        let envelope = match self.preview_envelope() {
            Some(e) => e,
            None => return,
        };
        let msg_id = &envelope.message_id;
        if self.preview_cache.get(msg_id, width).is_some() {
            return;
        }
        match mime_render::render_message(&envelope.path, width) {
            Ok(text) => self.preview_cache.insert(msg_id.clone(), width, text),
            Err(e) => self.preview_cache.insert(
                msg_id.clone(),
                width,
                format!("[Error rendering message: {}]", e),
            ),
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_time = Some(Instant::now());
    }

    fn clear_stale_status(&mut self) {
        if let Some(t) = self.status_time {
            if t.elapsed() > Duration::from_secs(3) {
                self.status_message = None;
                self.status_time = None;
            }
        }
    }

    fn rebuild_conversations(&mut self) {
        self.conversations = group_into_conversations(&self.envelopes);
    }

    /// Number of visible rows: conversations or envelopes depending on mode.
    fn visible_count(&self) -> usize {
        if self.conversations_mode {
            self.conversations.len()
        } else {
            self.envelopes.len()
        }
    }

    /// The envelope to show in the preview pane.
    fn preview_envelope(&self) -> Option<&Envelope> {
        if self.conversations_mode {
            self.conversations
                .get(self.selected)
                .map(|c| c.representative())
        } else {
            self.envelopes.get(self.selected)
        }
    }

    fn filter_description(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.filter_unread {
            parts.push("unread");
        }
        if self.filter_starred {
            parts.push("starred");
        }
        if self.filter_needs_reply {
            parts.push("needs-reply");
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("+"))
        }
    }

    // ── Navigation ──────────────────────────────────────────────────

    fn move_down(&mut self) {
        if self.selected + 1 < self.visible_count() {
            self.selected += 1;
            self.preview_scroll = 0;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.preview_scroll = 0;
        }
    }

    // ── Triage ──────────────────────────────────────────────────────

    /// Resolve a move target string to (maildir_path, human_description).
    ///
    /// If `target` starts with `/`, it's a literal maildir path.
    /// Otherwise it's an alias (archive, trash, spam, inbox, sent, drafts)
    /// resolved from the active account's folder config.
    fn resolve_move_target(&self, target: &str) -> (String, String) {
        if target.starts_with('/') {
            let desc = format!("Moved to {}", target);
            return (target.to_string(), desc);
        }
        let folders = self
            .account()
            .map(|a| &a.folders);
        let (path, desc) = match target {
            "archive" => (
                folders.map(|f| f.archive.clone()).unwrap_or_else(|| "/Archive".into()),
                "Archived".into(),
            ),
            "trash" => (
                folders.map(|f| f.trash.clone()).unwrap_or_else(|| "/Trash".into()),
                "Trashed".into(),
            ),
            "spam" => (
                folders.map(|f| f.spam.clone()).unwrap_or_else(|| "/Spam".into()),
                "Marked as spam".into(),
            ),
            "inbox" => (
                folders.map(|f| f.inbox.clone()).unwrap_or_else(|| "/Inbox".into()),
                "Moved to inbox".into(),
            ),
            "sent" => (
                folders.map(|f| f.sent.clone()).unwrap_or_else(|| "/Sent".into()),
                "Moved to sent".into(),
            ),
            "drafts" => (
                folders.map(|f| f.drafts.clone()).unwrap_or_else(|| "/Drafts".into()),
                "Moved to drafts".into(),
            ),
            other => {
                // Unknown alias — treat as literal path
                (format!("/{}", other), format!("Moved to /{}", other))
            }
        };
        (path, desc)
    }

    async fn triage_move(&mut self, dest_maildir: &str, desc: &str) -> Result<()> {
        let targets = self.triage_targets();
        if targets.is_empty() {
            return Ok(());
        }
        let count = targets.len();
        for (docid, maildir, flags) in &targets {
            let new_docid = self.mu.move_msg(*docid, Some(dest_maildir), None).await?;
            self.undo_stack.push(UndoEntry {
                action: UndoAction::MoveMessage {
                    docid: new_docid,
                    original_maildir: maildir.clone(),
                    original_flags: flags.clone(),
                },
                description: desc.to_string(),
            });
        }
        let removed: HashSet<u32> = targets.iter().map(|(d, _, _)| *d).collect();
        self.envelopes.retain(|e| !removed.contains(&e.docid));
        self.invalidate_folder_cache();
        self.rebuild_conversations();
        self.selected_set.clear();
        self.clamp_selection();
        self.preview_scroll = 0;
        self.set_status(format!("{} {} message(s)", desc, count));
        Ok(())
    }

    async fn triage_toggle_flag(&mut self, flag_char: char, desc: &str) -> Result<()> {
        let targets = self.triage_targets();
        if targets.is_empty() {
            return Ok(());
        }
        let count = targets.len();
        for (docid, maildir, flags) in &targets {
            let new_flags = if flags.contains(flag_char) {
                flags.replace(flag_char, "")
            } else {
                format!("{}{}", flags, flag_char)
            };
            let new_docid = self.mu.move_msg(*docid, None, Some(&new_flags)).await?;
            self.undo_stack.push(UndoEntry {
                action: UndoAction::MoveMessage {
                    docid: new_docid,
                    original_maildir: maildir.clone(),
                    original_flags: flags.clone(),
                },
                description: format!("toggle {}", desc),
            });
            if let Some(e) = self.envelopes.iter_mut().find(|e| e.docid == *docid) {
                e.docid = new_docid;
                e.flags = flags_from_string(&new_flags);
            }
        }
        self.invalidate_folder_cache();
        self.selected_set.clear();
        self.set_status(format!("Toggled {} on {} message(s)", desc, count));
        Ok(())
    }

    fn triage_targets(&self) -> Vec<(u32, String, String)> {
        if !self.selected_set.is_empty() {
            self.envelopes
                .iter()
                .filter(|e| self.selected_set.contains(&e.docid))
                .map(|e| (e.docid, e.maildir.clone(), e.flags_string()))
                .collect()
        } else if self.conversations_mode {
            // In conversations mode, act on all messages in the selected conversation
            if let Some(convo) = self.conversations.get(self.selected) {
                convo
                    .messages
                    .iter()
                    .map(|e| (e.docid, e.maildir.clone(), e.flags_string()))
                    .collect()
            } else {
                vec![]
            }
        } else if let Some(e) = self.envelopes.get(self.selected) {
            vec![(e.docid, e.maildir.clone(), e.flags_string())]
        } else {
            vec![]
        }
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_count();
        if count > 0 && self.selected >= count {
            self.selected = count - 1;
        }
    }

    async fn undo(&mut self) -> Result<()> {
        if let Some(entry) = self.undo_stack.pop() {
            match entry.action {
                UndoAction::MoveMessage {
                    docid,
                    original_maildir,
                    original_flags,
                } => {
                    let flags = if original_flags.is_empty() {
                        None
                    } else {
                        Some(original_flags.as_str())
                    };
                    self.mu
                        .move_msg(docid, Some(&original_maildir), flags)
                        .await?;
                    self.invalidate_folder_cache();
                    self.load_folder().await?;
                }
                UndoAction::DeleteSmartFolder { folder } => {
                    self.smart_folders.push(folder.clone());
                    smart_folders::save_smart_folders(&self.smart_folders, self.account_name());
                    let key = format!("@{}", folder.name);
                    self.smart_folder_queries
                        .insert(key.clone(), folder.query);
                    self.known_folders.push(key);
                    self.known_folders.sort();
                    self.rebuild_tabs();
                }
                UndoAction::DeleteSplit { split } => {
                    self.splits.push(split.clone());
                    splits::save_splits(&self.splits, self.account_name());
                    let key = format!("#{}", split.name);
                    self.split_queries.insert(key.clone(), split.query);
                    self.known_folders.push(key);
                    self.known_folders.sort();
                    self.rebuild_tabs();
                    self.refresh_split_caches().await;
                    if self.is_inbox_folder() {
                        self.load_folder().await?;
                    }
                }
                UndoAction::DeleteMaildirFolder { path } => {
                    // Re-create the maildir directory structure
                    if let Some(account) = self.account() {
                        let root = expand_maildir_root(&account.maildir);
                        let full = format!("{}{}", root, path);
                        let _ = std::fs::create_dir_all(format!("{}/cur", full));
                        let _ = std::fs::create_dir_all(format!("{}/new", full));
                        let _ = std::fs::create_dir_all(format!("{}/tmp", full));
                        self.known_folders.push(path);
                        self.known_folders.sort();
                    }
                }
            }
            self.set_status(format!("Undone: {}", entry.description));
        } else {
            self.set_status("Nothing to undo");
        }
        Ok(())
    }

    // ── Account switching ────────────────────────────────────────────

    async fn switch_account(&mut self, index: usize) -> Result<()> {
        if index == self.active_account {
            return Ok(());
        }
        if index >= self.config.accounts.len() {
            return Ok(());
        }

        let old_active = self.active_account;

        // Swap mu servers: current primary → background, background → primary
        if let Some(new_mu) = self.background_mu.remove(&index) {
            debug_log!("switch_account: swapping mu servers (bg for account {})", index);
            let old_mu = std::mem::replace(&mut self.mu, new_mu);
            self.background_mu.insert(old_active, old_mu);
        } else {
            // No background server for this account — fall back to quit/restart
            debug_log!("switch_account: no background server, doing quit/restart");
            self.mu.quit().await?;
            let muhome = self.config.effective_muhome(index);
            if let Some(account) = self.config.accounts.get(index) {
                let account_name = account.name.clone();
                let maildir = account.maildir.clone();
                let db_dir = muhome.as_deref().map(|p| std::path::PathBuf::from(p).join("xapian"));
                if db_dir.as_ref().is_some_and(|d| !d.is_dir()) {
                    self.set_status(format!("Initializing mu database for {}...", account_name));
                }
                crate::mu_client::ensure_mu_database(muhome.as_deref(), &maildir).await?;
            }
            self.mu = MuClient::start(muhome.as_deref()).await?;
        }

        // Update active account
        self.active_account = index;

        // Clear state
        self.envelopes.clear();
        self.thread_messages.clear();
        self.selected_set.clear();
        self.undo_stack = UndoStack::new();
        self.thread_selected = 0;
        self.thread_scroll = 0;
        self.selected = 0;
        self.scroll_offset = 0;
        self.preview_scroll = 0;

        // Reload smart folders for new account
        let acct_name = self.account_name().to_string();
        self.smart_folders = smart_folders::load_smart_folders(&acct_name);
        self.smart_folder_queries = self.smart_folders
            .iter()
            .map(|sf| (format!("@{}", sf.name), sf.query.clone()))
            .collect();

        // Rebuild known_folders
        self.known_folders = vec![
            "/Inbox".into(),
            "/Archive".into(),
            "/Drafts".into(),
            "/Sent".into(),
            "/Trash".into(),
            "/Junk".into(),
        ];
        for sf in &self.smart_folders {
            self.known_folders.push(format!("@{}", sf.name));
        }

        // Reload splits for new account
        self.splits = splits::load_splits(&acct_name);
        self.split_queries = self.splits
            .iter()
            .map(|s| (format!("#{}", s.name), s.query.clone()))
            .collect();
        self.split_excluded.clear();
        for s in &self.splits {
            self.known_folders.push(format!("#{}", s.name));
        }

        // Rebuild tabs for new account
        self.rebuild_tabs();

        // Navigate to new account's inbox
        let inbox = self.account()
            .map(|a| a.folders.inbox.clone())
            .unwrap_or_else(|| "/Inbox".to_string());
        self.current_folder = inbox;
        self.load_folder().await?;

        // Refresh split caches for the new account
        self.refresh_split_caches().await;
        if self.is_inbox_folder() && !self.split_excluded.is_empty() {
            self.load_folder().await?;
        }

        let name = self.account().map(|a| a.name.as_str()).unwrap_or("?");
        self.set_status(format!("Switched to {}", name));
        Ok(())
    }

    // ── Folder switching ────────────────────────────────────────────

    async fn navigate_folder(&mut self, folder: &str) -> Result<()> {
        self.previous_folder = Some(self.current_folder.clone());
        self.current_folder = folder.to_string();
        self.filter_unread = false;
        self.filter_starred = false;
        self.filter_needs_reply = false;
        self.adjust_tab_scroll();
        self.load_folder().await?;
        self.set_status(format!("Switched to {}", folder));
        Ok(())
    }

    /// Return the folder `delta` positions from the current one in the
    /// sorted known_folders list, wrapping around.
    fn next_folder(&self, delta: i32) -> Option<String> {
        let list = if self.tabs.is_empty() { &self.known_folders } else { &self.tabs };
        if list.is_empty() {
            return None;
        }
        let cur = list
            .iter()
            .position(|f| f == &self.current_folder)
            .unwrap_or(0);
        let len = list.len() as i32;
        let next = ((cur as i32 + delta) % len + len) % len;
        Some(list[next as usize].clone())
    }

    // ── Search ──────────────────────────────────────────────────────

    async fn execute_search(&mut self) -> Result<()> {
        if self.search_input.is_empty() {
            self.mode = InputMode::Normal;
            return Ok(());
        }
        // Add to history (avoid consecutive duplicates)
        if self.search_history.last() != Some(&self.search_input) {
            self.search_history.push(self.search_input.clone());
        }
        self.search_history_index = None;
        self.previous_folder = Some(self.current_folder.clone());
        self.current_folder = self.search_input.clone();
        self.mode = InputMode::Normal;
        self.load_folder().await?;
        self.set_status(format!("Search: {}", self.search_input));
        Ok(())
    }

    // ── Smart folder creation helpers ────────────────────────────────

    async fn update_smart_create_preview(&mut self) {
        if smart_folders::should_search(&self.smart_create_query) {
            match self.mu.find_preview(&self.smart_create_query, 5).await {
                Ok((envelopes, count)) => {
                    self.smart_create_count = Some(count);
                    self.smart_create_preview = envelopes
                        .iter()
                        .map(|e| e.subject.clone())
                        .collect();
                }
                Err(_) => {
                    self.smart_create_count = Some(0);
                    self.smart_create_preview.clear();
                }
            }
        } else {
            self.smart_create_count = None;
            self.smart_create_preview.clear();
        }
    }

    async fn delete_selected_folder(&mut self) {
        let filtered = self.filtered_folders();
        let folder = match filtered.get(self.folder_selected) {
            Some(f) => f.clone(),
            None => return,
        };

        if folder.starts_with("+ ") {
            // Can't delete special entries
            return;
        }

        if let Some(name) = folder.strip_prefix('@') {
            // Smart folder — remove from list and save
            if let Some(pos) = self.smart_folders.iter().position(|sf| sf.name == name) {
                let removed = self.smart_folders.remove(pos);
                smart_folders::save_smart_folders(&self.smart_folders, self.account_name());
                self.smart_folder_queries.remove(&folder);
                self.known_folders.retain(|f| f != &folder);
                self.rebuild_tabs();
                self.undo_stack.push(UndoEntry {
                    action: UndoAction::DeleteSmartFolder { folder: removed },
                    description: format!("Deleted smart folder {}", name),
                });
                self.set_status(format!("Deleted smart folder \"{}\" (z to undo)", name));
                // Clamp selection
                let max = self.filtered_folders().len();
                if self.folder_selected >= max && max > 0 {
                    self.folder_selected = max - 1;
                }
            }
        } else if let Some(name) = folder.strip_prefix('#') {
            // Split — remove from list and save
            if let Some(pos) = self.splits.iter().position(|s| s.name == name) {
                let removed = self.splits.remove(pos);
                splits::save_splits(&self.splits, self.account_name());
                self.split_queries.remove(&folder);
                self.known_folders.retain(|f| f != &folder);
                self.rebuild_tabs();
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
        } else if folder.starts_with('/') {
            // Maildir — check if empty, then delete
            if let Some(account) = self.account() {
                let root = expand_maildir_root(&account.maildir);
                let full = format!("{}{}", root, folder);
                let full_path = std::path::PathBuf::from(&full);

                // Check if maildir is empty (no files in cur/, new/, tmp/)
                let is_empty = ["cur", "new", "tmp"].iter().all(|sub| {
                    let sub_dir = full_path.join(sub);
                    match std::fs::read_dir(&sub_dir) {
                        Ok(entries) => entries
                            .filter_map(|e| e.ok())
                            .all(|e| !e.path().is_file()),
                        Err(_) => true,
                    }
                });

                if is_empty {
                    // Delete the directory
                    let _ = std::fs::remove_dir_all(&full_path);
                    self.known_folders.retain(|f| f != &folder);
                    self.undo_stack.push(UndoEntry {
                        action: UndoAction::DeleteMaildirFolder {
                            path: folder.clone(),
                        },
                        description: format!("Deleted folder {}", folder),
                    });
                    self.set_status(format!("Deleted folder \"{}\" (z to undo)", folder));
                    let max = self.filtered_folders().len();
                    if self.folder_selected >= max && max > 0 {
                        self.folder_selected = max - 1;
                    }
                } else {
                    self.set_status("Folder not empty, cannot delete");
                }
            }
        }
    }

    // ── Thread view ─────────────────────────────────────────────────

    async fn open_thread(&mut self) -> Result<()> {
        let envelope = match self.preview_envelope() {
            Some(e) => e.clone(),
            None => return Ok(()),
        };
        let query = format!("msgid:{}", envelope.message_id);
        let opts = FindOpts {
            threads: true,
            include_related: true,
            descending: false,
            ..Default::default()
        };
        let thread_envelopes = self.mu.find(&query, &opts).await.unwrap_or_default();
        if thread_envelopes.is_empty() {
            self.thread_messages = vec![ThreadMessage {
                envelope: envelope.clone(),
                body: None,
                expanded: true,
            }];
        } else {
            self.thread_messages = thread_envelopes
                .into_iter()
                .map(|e| {
                    let is_selected = e.message_id == envelope.message_id;
                    ThreadMessage {
                        envelope: e,
                        body: None,
                        expanded: is_selected,
                    }
                })
                .collect();
        }
        self.thread_selected = self
            .thread_messages
            .iter()
            .position(|m| m.envelope.message_id == envelope.message_id)
            .unwrap_or(0);
        self.thread_scroll = 0;
        self.mode = InputMode::ThreadView;
        Ok(())
    }

    fn ensure_thread_body_loaded(&mut self, width: u16) {
        for msg in &mut self.thread_messages {
            if msg.expanded && msg.body.is_none() {
                match mime_render::render_message(&msg.envelope.path, width) {
                    Ok(text) => msg.body = Some(text),
                    Err(e) => msg.body = Some(format!("[Error: {}]", e)),
                }
            }
        }
    }

    // ── Multi-select ────────────────────────────────────────────────

    fn toggle_select(&mut self) {
        if self.conversations_mode {
            // Select/deselect all docids in the conversation
            if let Some(convo) = self.conversations.get(self.selected) {
                let docids = convo.all_docids();
                let all_selected = docids.iter().all(|d| self.selected_set.contains(d));
                if all_selected {
                    for d in docids {
                        self.selected_set.remove(&d);
                    }
                } else {
                    for d in docids {
                        self.selected_set.insert(d);
                    }
                }
            }
        } else if let Some(e) = self.envelopes.get(self.selected) {
            let docid = e.docid;
            if self.selected_set.contains(&docid) {
                self.selected_set.remove(&docid);
            } else {
                self.selected_set.insert(docid);
            }
        }
    }

    // ── Compose helpers ─────────────────────────────────────────────

    fn build_compose_context(
        &self,
        kind: &compose::ComposeKind,
    ) -> Option<compose::ComposeContext> {
        match kind {
            compose::ComposeKind::NewMessage => Some(compose::ComposeContext::new_message()),
            compose::ComposeKind::Reply => {
                let envelope = self.selected_envelope()?;
                let body_text =
                    mime_render::render_message(&envelope.path, 80).unwrap_or_default();
                Some(compose::ComposeContext::reply(envelope, &body_text, false))
            }
            compose::ComposeKind::ReplyAll => {
                let envelope = self.selected_envelope()?;
                let body_text =
                    mime_render::render_message(&envelope.path, 80).unwrap_or_default();
                Some(compose::ComposeContext::reply(envelope, &body_text, true))
            }
            compose::ComposeKind::Forward => {
                let envelope = self.selected_envelope()?;
                let body_text =
                    mime_render::render_message(&envelope.path, 80).unwrap_or_default();
                Some(compose::ComposeContext::forward(envelope, &body_text))
            }
        }
    }

    // ── Filtered list helpers ───────────────────────────────────────

    fn filtered_folders(&self) -> Vec<String> {
        let filter = self.folder_filter.to_lowercase();
        let mut result = Vec::new();
        // Special entries always at top (not affected by filter)
        result.push("+ New smart folder".to_string());
        result.push("+ New split".to_string());
        result.push("+ New maildir folder".to_string());
        // Then filtered known folders
        for f in &self.known_folders {
            if filter.is_empty() {
                result.push(f.clone());
            } else {
                // For smart folders (@Name) and splits (#Name), also match against just the name
                let matches = f.to_lowercase().contains(&filter)
                    || f.strip_prefix('@')
                        .is_some_and(|name| name.to_lowercase().contains(&filter))
                    || f.strip_prefix('#')
                        .is_some_and(|name| name.to_lowercase().contains(&filter));
                if matches {
                    result.push(f.clone());
                }
            }
        }
        result
    }

    /// Like filtered_folders() but without the special "+ New ..." entries.
    /// Used for MoveToFolder where those entries don't apply.
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

    fn filtered_palette(&self) -> Vec<PaletteEntry> {
        let filter = self.palette_filter.to_lowercase();
        self.palette_entries
            .iter()
            .filter(|e| {
                filter.is_empty()
                    || e.name.to_lowercase().contains(&filter)
                    || e.description.to_lowercase().contains(&filter)
            })
            .cloned()
            .collect()
    }

    // ── IPC command handling ──────────────────────────────────────────

    /// If the IPC command specifies an account, switch to it first.
    async fn switch_to_account_if_needed(&mut self, account: &Option<String>) -> Result<()> {
        if let Some(name) = account {
            if let Some(idx) = self.config.accounts.iter().position(|a| a.name == *name) {
                if idx != self.active_account {
                    self.switch_account(idx).await?;
                }
            } else {
                self.set_status(format!("Unknown account: {}", name));
            }
        }
        Ok(())
    }

    async fn handle_ipc_command(&mut self, cmd: IpcCommand) -> Result<()> {
        debug_log!("handle_ipc_command: {:?}", cmd);
        match cmd {
            IpcCommand::Open(url_serde) => {
                let url: HuttUrl = url_serde.into();
                match url {
                    HuttUrl::Message { id, account } => {
                        self.switch_to_account_if_needed(&account).await?;
                        let query = format!("msgid:{}", id);
                        debug_log!("IPC Message: query={}", query);
                        self.mode = InputMode::Normal;
                        self.thread_messages.clear();
                        self.current_folder = query;
                        match self.load_folder().await {
                            Ok(()) => debug_log!("IPC Message: loaded {} envelopes", self.envelopes.len()),
                            Err(e) => debug_log!("IPC Message: load error: {}", e),
                        }
                        self.set_status(format!("Opened message {}", id));
                    }
                    HuttUrl::Thread { id, account } => {
                        self.switch_to_account_if_needed(&account).await?;
                        let query = format!("msgid:{}", id);
                        debug_log!("IPC Thread: query={}", query);
                        self.mode = InputMode::Normal;
                        self.thread_messages.clear();
                        let result = self
                            .mu
                            .find(&query, &crate::mu_client::FindOpts::default())
                            .await;
                        debug_log!("IPC Thread: find result: {:?}", result.as_ref().map(|v| v.len()));
                        let envelopes = result.unwrap_or_default();
                        if let Some(envelope) = envelopes.into_iter().next() {
                            self.envelopes = vec![envelope];
                            self.selected = 0;
                            match self.open_thread().await {
                                Ok(()) => debug_log!("IPC Thread: opened, {} messages", self.thread_messages.len()),
                                Err(e) => debug_log!("IPC Thread: open_thread error: {}", e),
                            }
                            self.set_status(format!("Opened thread {}", id));
                        } else {
                            debug_log!("IPC Thread: message not found");
                            self.set_status(format!("Message not found: {}", id));
                        }
                    }
                    HuttUrl::Search { query, account } => {
                        self.switch_to_account_if_needed(&account).await?;
                        debug_log!("IPC Search: query={}", query);
                        self.mode = InputMode::Normal;
                        self.thread_messages.clear();
                        self.current_folder = query.clone();
                        match self.load_folder().await {
                            Ok(()) => debug_log!("IPC Search: loaded {} envelopes", self.envelopes.len()),
                            Err(e) => debug_log!("IPC Search: load error: {}", e),
                        }
                        self.set_status(format!("Search: {}", query));
                    }
                    HuttUrl::Compose { to, subject, account } => {
                        self.switch_to_account_if_needed(&account).await?;
                        let mut ctx = compose::ComposeContext::new_message();
                        ctx.to = vec![crate::envelope::Address {
                            name: None,
                            email: to,
                        }];
                        ctx.subject = subject;
                        self.compose_pending =
                            Some(compose::ComposePending::Ready(ctx));
                        self.set_status("Compose from URL");
                    }
                }
            }
            IpcCommand::Navigate { folder, account } => {
                self.switch_to_account_if_needed(&account).await?;
                debug_log!("IPC Navigate: folder={}", folder);
                self.mode = InputMode::Normal;
                self.thread_messages.clear();
                match self.navigate_folder(&folder).await {
                    Ok(()) => debug_log!("IPC Navigate: loaded {} envelopes", self.envelopes.len()),
                    Err(e) => debug_log!("IPC Navigate: error: {}", e),
                }
            }
            IpcCommand::Quit => {
                self.should_quit = true;
            }
        }
        Ok(())
    }

    // ── Action dispatch ─────────────────────────────────────────────

    async fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            // Navigation
            Action::MoveDown => self.move_down(),
            Action::MoveUp => self.move_up(),
            Action::JumpTop => {
                self.selected = 0;
                self.preview_scroll = 0;
            }
            Action::JumpBottom => {
                let count = self.visible_count();
                if count > 0 {
                    self.selected = count - 1;
                    self.preview_scroll = 0;
                }
            }
            Action::ScrollPreviewDown => match self.mode {
                InputMode::ThreadView => {
                    self.thread_scroll = self.thread_scroll.saturating_add(5);
                }
                InputMode::Help => {
                    self.help_scroll = self.help_scroll.saturating_add(3);
                }
                _ => {
                    self.preview_scroll = self.preview_scroll.saturating_add(5);
                }
            },
            Action::ScrollPreviewUp => match self.mode {
                InputMode::ThreadView => {
                    self.thread_scroll = self.thread_scroll.saturating_sub(5);
                }
                InputMode::Help => {
                    self.help_scroll = self.help_scroll.saturating_sub(3);
                }
                _ => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(5);
                }
            },
            Action::HalfPageDown => {
                let count = self.visible_count();
                let max = if count == 0 { 0 } else { count - 1 };
                self.selected = (self.selected + 10).min(max);
                self.preview_scroll = 0;
            }
            Action::HalfPageUp => {
                self.selected = self.selected.saturating_sub(10);
                self.preview_scroll = 0;
            }
            Action::FullPageDown => {
                let count = self.visible_count();
                let max = if count == 0 { 0 } else { count - 1 };
                self.selected = (self.selected + 20).min(max);
                self.preview_scroll = 0;
            }
            Action::FullPageUp => {
                self.selected = self.selected.saturating_sub(20);
                self.preview_scroll = 0;
            }

            // Triage — move to folder (alias, literal path, or picker)
            Action::MoveToFolder(ref target) => {
                if let Some(dest) = target {
                    let (maildir, desc) = self.resolve_move_target(dest);
                    self.triage_move(&maildir, &desc).await?;
                } else if !self.triage_targets().is_empty() {
                    self.folder_filter.clear();
                    self.folder_selected = 0;
                    self.mode = InputMode::MoveToFolder;
                }
            }
            Action::ToggleRead => self.triage_toggle_flag('S', "read/unread").await?,
            Action::ToggleStar => self.triage_toggle_flag('F', "star").await?,
            Action::Undo => self.undo().await?,

            // Folder switching
            Action::GoInbox => {
                let (path, _) = self.resolve_move_target("inbox");
                self.navigate_folder(&path).await?;
            }
            Action::GoArchive => {
                let (path, _) = self.resolve_move_target("archive");
                self.navigate_folder(&path).await?;
            }
            Action::GoDrafts => {
                let (path, _) = self.resolve_move_target("drafts");
                self.navigate_folder(&path).await?;
            }
            Action::GoSent => {
                let (path, _) = self.resolve_move_target("sent");
                self.navigate_folder(&path).await?;
            }
            Action::GoTrash => {
                let (path, _) = self.resolve_move_target("trash");
                self.navigate_folder(&path).await?;
            }
            Action::GoSpam => {
                let (path, _) = self.resolve_move_target("spam");
                self.navigate_folder(&path).await?;
            }
            Action::GoFolderPicker => {
                self.folder_filter.clear();
                self.folder_selected = 0;
                self.mode = InputMode::FolderPicker;
            }

            // Folder cycling
            Action::NextFolder => {
                if let Some(folder) = self.next_folder(1) {
                    self.navigate_folder(&folder).await?;
                }
            }
            Action::PrevFolder => {
                if let Some(folder) = self.next_folder(-1) {
                    self.navigate_folder(&folder).await?;
                }
            }

            // Account switching
            Action::NextAccount => {
                if self.config.accounts.len() > 1 {
                    let next = (self.active_account + 1) % self.config.accounts.len();
                    self.switch_account(next).await?;
                }
            }
            Action::PrevAccount => {
                if self.config.accounts.len() > 1 {
                    let prev = if self.active_account == 0 {
                        self.config.accounts.len() - 1
                    } else {
                        self.active_account - 1
                    };
                    self.switch_account(prev).await?;
                }
            }

            // Search
            Action::EnterSearch => {
                // Pre-fill with the current view's query
                self.search_input = if let Some(q) =
                    self.smart_folder_queries.get(&self.current_folder)
                {
                    format!("{} ", q)
                } else if self.current_folder.starts_with('/') {
                    format!("maildir:{} ", self.current_folder)
                } else {
                    format!("{} ", self.current_folder)
                };
                self.search_history_index = None;
                self.mode = InputMode::Search;
            }

            // Filters
            Action::FilterUnread => {
                self.filter_unread = !self.filter_unread;
                self.load_folder().await?;
            }
            Action::FilterStarred => {
                self.filter_starred = !self.filter_starred;
                self.load_folder().await?;
            }
            Action::FilterNeedsReply => {
                self.filter_needs_reply = !self.filter_needs_reply;
                self.load_folder().await?;
            }

            // Multi-select
            Action::ToggleSelect => {
                self.toggle_select();
                self.move_down();
            }
            Action::SelectDown => {
                self.toggle_select();
                self.move_down();
            }
            Action::SelectUp => {
                self.toggle_select();
                self.move_up();
            }

            // Thread view
            Action::OpenThread => self.open_thread().await?,
            Action::CloseThread => {
                self.mode = InputMode::Normal;
                self.thread_messages.clear();
            }
            Action::ThreadNext => {
                if self.thread_selected + 1 < self.thread_messages.len() {
                    self.thread_selected += 1;
                }
            }
            Action::ThreadPrev => {
                if self.thread_selected > 0 {
                    self.thread_selected -= 1;
                }
            }
            Action::ThreadToggleExpand => {
                if let Some(msg) = self.thread_messages.get_mut(self.thread_selected) {
                    msg.expanded = !msg.expanded;
                }
            }
            Action::ThreadExpandAll => {
                let all_expanded = self.thread_messages.iter().all(|m| m.expanded);
                for msg in &mut self.thread_messages {
                    msg.expanded = !all_expanded;
                }
            }

            // Compose
            Action::Compose => self.compose_pending = Some(compose::ComposePending::Kind(compose::ComposeKind::NewMessage)),
            Action::Reply => self.compose_pending = Some(compose::ComposePending::Kind(compose::ComposeKind::Reply)),
            Action::ReplyAll => self.compose_pending = Some(compose::ComposePending::Kind(compose::ComposeKind::ReplyAll)),
            Action::Forward => self.compose_pending = Some(compose::ComposePending::Kind(compose::ComposeKind::Forward)),

            // Linkability
            Action::CopyMessageUrl => {
                if let Some(e) = self.selected_envelope() {
                    let url = links::format_message_url(&e.message_id);
                    match links::copy_to_clipboard(&url) {
                        Ok(()) => self.set_status("Message URL copied"),
                        Err(e) => self.set_status(format!("Clipboard error: {}", e)),
                    }
                }
            }
            Action::CopyThreadUrl => {
                if let Some(e) = self.selected_envelope() {
                    let url = links::format_thread_url(&e.message_id);
                    match links::copy_to_clipboard(&url) {
                        Ok(()) => self.set_status("Thread URL copied"),
                        Err(e) => self.set_status(format!("Clipboard error: {}", e)),
                    }
                }
            }
            Action::OpenInBrowser => {
                if let Some(e) = self.selected_envelope() {
                    let path = e.path.clone();
                    match std::fs::read(&path) {
                        Ok(raw) => {
                            if let Some(msg) = mail_parser::MessageParser::default().parse(&raw) {
                                if let Some(html) = msg.body_html(0) {
                                    let _ = links::open_html_in_browser(html.as_bytes());
                                    self.set_status("Opened in browser");
                                } else {
                                    self.set_status("No HTML content");
                                }
                            }
                        }
                        Err(e) => self.set_status(format!("Read error: {}", e)),
                    }
                }
            }

            // Conversations
            Action::ToggleConversations => {
                self.conversations_mode = !self.conversations_mode;
                self.selected = 0;
                self.scroll_offset = 0;
                self.preview_scroll = 0;
                self.selected_set.clear();
                self.rebuild_conversations();
                if self.conversations_mode {
                    self.set_status("Conversations view");
                } else {
                    self.set_status("Message view");
                }
            }

            // Help
            Action::ShowHelp => {
                self.help_scroll = 0;
                self.mode = InputMode::Help;
            }

            // Command palette
            Action::OpenCommandPalette => {
                self.palette_filter.clear();
                self.palette_selected = 0;
                self.palette_entries = PaletteEntry::all_actions();
                self.mode = InputMode::CommandPalette;
            }

            // Sync — runs sync_command in background, then reindexes
            Action::SyncMail => {
                if let Some(cmd) = self.config.effective_sync_command(self.active_account).map(|s| s.to_string()) {
                    self.set_status(format!("Syncing: {}...", cmd));
                    let tx = self.shell_tx.clone();
                    tokio::spawn(async move {
                        let output = tokio::process::Command::new("sh")
                            .args(["-c", &cmd])
                            .output()
                            .await;
                        match output {
                            Ok(o) => {
                                let _ = tx.send(Ok(ShellResult {
                                    command: cmd,
                                    reindex: true,
                                    stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                                    stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                                    status: o.status,
                                }));
                            }
                            Err(e) => {
                                let _ = tx.send(Err(ShellError {
                                    command: cmd,
                                    error: e.to_string(),
                                }));
                            }
                        }
                    });
                } else {
                    self.set_status("No sync_command configured");
                }
            }

            Action::OpenAccountPicker => {
                if self.config.accounts.len() > 1 {
                    self.account_picker_selected = self.active_account;
                    self.mode = InputMode::AccountPicker;
                }
            }

            Action::CreateSplit => {
                self.smart_create_query.clear();
                self.smart_create_name.clear();
                self.smart_create_phase = 0;
                self.smart_create_preview.clear();
                self.smart_create_count = None;
                self.creating_split = true;
                self.mode = InputMode::SmartFolderCreate;
            }

            // Text input
            Action::InputChar(c) => match self.mode {
                InputMode::Search => self.search_input.push(c),
                InputMode::FolderPicker => {
                    self.folder_filter.push(c);
                    // Skip past the two special entries to first real folder
                    self.folder_selected = 2;
                }
                InputMode::MoveToFolder => {
                    self.folder_filter.push(c);
                    self.folder_selected = 0;
                }
                InputMode::CommandPalette => {
                    self.palette_filter.push(c);
                    self.palette_selected = 0;
                }
                InputMode::SmartFolderCreate => {
                    self.smart_create_query.push(c);
                    self.update_smart_create_preview().await;
                }
                InputMode::SmartFolderName => {
                    self.smart_create_name.push(c);
                }
                InputMode::MaildirCreate => {
                    self.maildir_create_input.push(c);
                }
                _ => {}
            },
            Action::InputBackspace => match self.mode {
                InputMode::Search => {
                    self.search_input.pop();
                }
                InputMode::FolderPicker => {
                    self.folder_filter.pop();
                    self.folder_selected = 2;
                }
                InputMode::MoveToFolder => {
                    self.folder_filter.pop();
                    self.folder_selected = 0;
                }
                InputMode::CommandPalette => {
                    self.palette_filter.pop();
                    self.palette_selected = 0;
                }
                InputMode::SmartFolderCreate => {
                    self.smart_create_query.pop();
                    self.update_smart_create_preview().await;
                }
                InputMode::SmartFolderName => {
                    self.smart_create_name.pop();
                }
                InputMode::MaildirCreate => {
                    self.maildir_create_input.pop();
                }
                _ => {}
            },
            Action::InputHistoryPrev => {
                if self.mode == InputMode::Search && !self.search_history.is_empty() {
                    let idx = match self.search_history_index {
                        None => self.search_history.len() - 1,
                        Some(0) => 0,
                        Some(i) => i - 1,
                    };
                    self.search_history_index = Some(idx);
                    self.search_input = self.search_history[idx].clone();
                }
            }
            Action::InputHistoryNext => {
                if self.mode == InputMode::Search {
                    if let Some(idx) = self.search_history_index {
                        if idx + 1 < self.search_history.len() {
                            self.search_history_index = Some(idx + 1);
                            self.search_input = self.search_history[idx + 1].clone();
                        } else {
                            self.search_history_index = None;
                            self.search_input.clear();
                        }
                    }
                }
            }
            Action::InputSubmit => match self.mode {
                InputMode::Search => self.execute_search().await?,
                InputMode::FolderPicker => {
                    let filtered = self.filtered_folders();
                    if let Some(folder) = filtered.get(self.folder_selected).cloned() {
                        if folder == "+ New smart folder" {
                            self.smart_create_query.clear();
                            self.smart_create_name.clear();
                            self.smart_create_phase = 0;
                            self.smart_create_preview.clear();
                            self.smart_create_count = None;
                            self.creating_split = false;
                            self.mode = InputMode::SmartFolderCreate;
                        } else if folder == "+ New split" {
                            self.smart_create_query.clear();
                            self.smart_create_name.clear();
                            self.smart_create_phase = 0;
                            self.smart_create_preview.clear();
                            self.smart_create_count = None;
                            self.creating_split = true;
                            self.mode = InputMode::SmartFolderCreate;
                        } else if folder == "+ New maildir folder" {
                            self.maildir_create_input.clear();
                            self.mode = InputMode::MaildirCreate;
                        } else {
                            self.mode = InputMode::Normal;
                            self.navigate_folder(&folder).await?;
                        }
                    }
                }
                InputMode::CommandPalette => {
                    let filtered = self.filtered_palette();
                    if let Some(entry) = filtered.get(self.palette_selected) {
                        let action = entry.action.clone();
                        self.mode = InputMode::Normal;
                        Box::pin(self.handle_action(action)).await?;
                    }
                }
                InputMode::SmartFolderCreate => {
                    if !self.smart_create_query.trim().is_empty() {
                        self.smart_create_name = self.smart_create_query.clone();
                        self.smart_create_phase = 1;
                        self.mode = InputMode::SmartFolderName;
                    }
                }
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
                            self.rebuild_tabs();
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
                            self.rebuild_tabs();
                            self.mode = InputMode::Normal;
                            self.navigate_folder(&key).await?;
                        }
                        self.creating_split = false;
                    }
                }
                InputMode::MaildirCreate => {
                    let path = self.maildir_create_input.trim().to_string();
                    if !path.is_empty() {
                        let folder_path = if path.starts_with('/') {
                            path.clone()
                        } else {
                            format!("/{}", path)
                        };
                        if let Some(account) = self.account() {
                            let root = expand_maildir_root(&account.maildir);
                            let full = format!("{}{}", root, folder_path);
                            let _ = std::fs::create_dir_all(format!("{}/cur", full));
                            let _ = std::fs::create_dir_all(format!("{}/new", full));
                            let _ = std::fs::create_dir_all(format!("{}/tmp", full));
                            self.known_folders.push(folder_path.clone());
                            self.known_folders.sort();
                            self.mode = InputMode::Normal;
                            self.navigate_folder(&folder_path).await?;
                        } else {
                            self.set_status("No account configured");
                            self.mode = InputMode::FolderPicker;
                        }
                    }
                }
                InputMode::MoveToFolder => {
                    let filtered = self.filtered_folders_plain();
                    if let Some(folder) = filtered.get(self.folder_selected).cloned() {
                        // Only move to real maildir folders (starting with /)
                        if folder.starts_with('/') {
                            self.mode = InputMode::Normal;
                            self.triage_move(&folder, &format!("Moved to {}", folder))
                                .await?;
                        } else {
                            self.set_status("Can only move to maildir folders");
                        }
                    }
                }
                _ => {}
            },
            Action::InputCancel => match self.mode {
                InputMode::Search => {
                    self.mode = InputMode::Normal;
                    if let Some(prev) = self.previous_folder.take() {
                        self.current_folder = prev;
                        self.load_folder().await?;
                    }
                }
                InputMode::FolderPicker | InputMode::CommandPalette | InputMode::MoveToFolder => {
                    self.mode = InputMode::Normal;
                }
                InputMode::Help => {
                    self.mode = InputMode::Normal;
                }
                InputMode::SmartFolderCreate => {
                    self.creating_split = false;
                    self.mode = InputMode::FolderPicker;
                }
                InputMode::SmartFolderName => {
                    // Go back to query phase
                    self.smart_create_phase = 0;
                    self.mode = InputMode::SmartFolderCreate;
                }
                InputMode::MaildirCreate => {
                    self.mode = InputMode::FolderPicker;
                }
                _ => {}
            },

            // Custom bindings: shell commands
            Action::RunShell {
                command,
                reindex,
                suspend,
            } => {
                if suspend {
                    // Deferred to run loop (needs terminal suspend/resume)
                    self.shell_pending = Some(ShellPending { command, reindex });
                } else {
                    // Spawn in background so the TUI stays responsive
                    self.set_status(format!("Running: {}...", command));
                    let tx = self.shell_tx.clone();
                    let cmd = command.clone();
                    tokio::spawn(async move {
                        let output = tokio::process::Command::new("sh")
                            .args(["-c", &cmd])
                            .output()
                            .await;
                        match output {
                            Ok(o) => {
                                let _ = tx.send(Ok(ShellResult {
                                    command: cmd,
                                    reindex,
                                    stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                                    stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                                    status: o.status,
                                }));
                            }
                            Err(e) => {
                                let _ = tx.send(Err(ShellError {
                                    command: cmd,
                                    error: e.to_string(),
                                }));
                            }
                        }
                    });
                }
            }

            // Custom bindings: folder navigation
            Action::NavigateFolder(folder) => {
                self.navigate_folder(&folder).await?;
            }

            // System
            Action::Redraw => {} // handled in run loop
            Action::Quit => self.should_quit = true,
            Action::Noop => {}
        }
        Ok(())
    }
}

/// Expand `~/` prefix in a maildir root path.
fn expand_maildir_root(maildir: &str) -> String {
    if let Some(rest) = maildir.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/{}", home, rest)
    } else {
        maildir.to_string()
    }
}

/// Save a formatted message to the Sent maildir folder.
fn save_to_sent(maildir_root: &str, sent_folder: &str, message: &[u8]) -> Result<()> {
    use anyhow::Context;
    let root = expand_maildir_root(maildir_root);
    let sent_cur = format!("{}{}/cur", root, sent_folder);

    // Ensure the Sent/cur directory exists
    std::fs::create_dir_all(&sent_cur)
        .with_context(|| format!("failed to create {}", sent_cur))?;

    // Maildir filename: time.pid_seq.hostname:2,S (Seen flag)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hostname = gethostname();
    let filename = format!(
        "{}.{}_{}.{}:2,S",
        timestamp,
        std::process::id(),
        rand_seq(),
        hostname,
    );
    let path = format!("{}/{}", sent_cur, filename);

    std::fs::write(&path, message).with_context(|| format!("failed to save to {}", path))?;

    Ok(())
}

/// Simple counter for unique maildir filenames within a process.
fn rand_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Get the system hostname (for maildir filenames).
fn gethostname() -> String {
    let mut buf = [0u8; 256];
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if ret == 0 {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).to_string()
    } else {
        "localhost".to_string()
    }
}

pub async fn run(mut app: App) -> Result<()> {
    app.load_folder().await?;

    // Populate split caches and re-filter inbox if needed
    app.refresh_split_caches().await;
    if app.is_inbox_folder() && !app.split_excluded.is_empty() {
        app.load_folder().await?;
    }

    // Spawn background mu servers for non-active accounts (for prefetch)
    if app.config.background_servers {
        for idx in 0..app.config.accounts.len() {
            if idx == app.active_account {
                continue;
            }
            let muhome = app.config.effective_muhome(idx);
            match MuClient::start(muhome.as_deref()).await {
                Ok(client) => {
                    let name = app.config.accounts[idx].name.as_str();
                    debug_log!("background mu: started for account {} ({:?})", name, muhome);
                    app.background_mu.insert(idx, client);
                }
                Err(e) => {
                    let name = app.config.accounts.get(idx).map(|a| a.name.as_str()).unwrap_or("?");
                    debug_log!("background mu: failed for account {} ({}), prefetch unavailable", name, e);
                }
            }
        }
    }

    // Start IPC listener as a background task, sending commands through a channel
    // Create shell result channel — replace the dummy one from App::new
    let (shell_tx, mut shell_rx) = tokio::sync::mpsc::unbounded_channel();
    app.shell_tx = shell_tx;

    let (ipc_tx, mut ipc_rx) = tokio::sync::mpsc::unbounded_channel::<IpcCommand>();
    let _ipc_guard = match IpcListener::bind() {
        Ok(listener) => {
            let tx = ipc_tx;
            Some(tokio::spawn(async move {
                debug_log!("IPC listener started");
                loop {
                    match listener.accept().await {
                        Ok(cmd) => {
                            debug_log!("IPC accepted: {:?}", cmd);
                            if tx.send(cmd).is_err() {
                                debug_log!("IPC channel closed, exiting");
                                break;
                            }
                        }
                        Err(e) => {
                            debug_log!("IPC accept error: {}", e);
                            continue;
                        }
                    }
                }
            }))
        }
        Err(e) => {
            eprintln!("IPC socket: {}", e);
            drop(ipc_tx); // drop sender so receiver never blocks
            None
        }
    };

    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    io::stdout().execute(crossterm::event::EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let sequence_timeout = Duration::from_millis(1000);
    let mut last_key_time = Instant::now();
    let mut event_stream = EventStream::new();

    loop {
        app.clear_stale_status();

        let preview_width = {
            let size = terminal.size()?;
            (size.width * (100 - app.list_pct) / 100).saturating_sub(4)
        };

        if app.mode == InputMode::ThreadView {
            app.ensure_thread_body_loaded(preview_width);
        } else {
            app.ensure_preview_loaded(preview_width);
        }

        terminal.draw(|frame| {
            let size = frame.area();
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(3),
                    Constraint::Length(1),
                ])
                .split(size);

            // Top bar
            let thread_subject = if app.mode == InputMode::ThreadView {
                app.thread_messages
                    .first()
                    .map(|m| m.envelope.subject.as_str())
            } else {
                None
            };
            let unread = app.envelopes.iter().filter(|e| e.is_unread()).count();
            let account_name = if app.config.accounts.len() > 1 {
                app.account().map(|a| a.name.as_str())
            } else {
                None
            };
            let top = TopBar {
                folder: &app.current_folder,
                unread_count: unread,
                total_count: app.visible_count(),
                mode: &app.mode,
                thread_subject,
                account_name,
                conversations_mode: app.conversations_mode,
                tabs: &app.tabs,
                tab_scroll: app.tab_scroll,
                multi_account: app.config.accounts.len() > 1,
            };
            let tab_bar_result = top.render_with_regions(outer[0], frame.buffer_mut());
            app.tab_regions = tab_bar_result.regions;

            // Content
            match app.mode {
                InputMode::ThreadView => {
                    let tv = ThreadView {
                        messages: &app.thread_messages,
                        selected: app.thread_selected,
                        scroll: app.thread_scroll,
                    };
                    frame.render_widget(tv, outer[1]);
                }
                _ => {
                    let content = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(app.list_pct), Constraint::Percentage(100 - app.list_pct)])
                        .split(outer[1]);

                    if app.conversations_mode {
                        let conv_list = ConversationList {
                            conversations: &app.conversations,
                            selected: app.selected,
                            offset: app.scroll_offset,
                            multi_selected: &app.selected_set,
                        };
                        frame.render_widget(conv_list, content[0]);

                        let height = content[0].height as usize;
                        let (new_offset, _) = EnvelopeList::visible_range(
                            app.selected,
                            app.scroll_offset,
                            height,
                            app.conversations.len(),
                        );
                        app.scroll_offset = new_offset;
                    } else {
                        let env_list = EnvelopeList {
                            envelopes: &app.envelopes,
                            selected: app.selected,
                            offset: app.scroll_offset,
                            multi_selected: &app.selected_set,
                        };
                        frame.render_widget(env_list, content[0]);

                        let height = content[0].height as usize;
                        let (new_offset, _) = EnvelopeList::visible_range(
                            app.selected,
                            app.scroll_offset,
                            height,
                            app.envelopes.len(),
                        );
                        app.scroll_offset = new_offset;
                    }

                    let envelope = app.preview_envelope();
                    let body = envelope
                        .and_then(|e| app.preview_cache.get(&e.message_id, preview_width));
                    let preview = PreviewPane {
                        envelope,
                        body,
                        scroll: app.preview_scroll,
                    };
                    frame.render_widget(preview, content[1]);
                }
            }

            // Bottom bar
            let filter_desc = app.filter_description();
            let bottom = BottomBar {
                mode: &app.mode,
                pending_key: app.keymap.pending_display(),
                search_input: if app.mode == InputMode::Search {
                    Some(&app.search_input)
                } else {
                    None
                },
                status_message: app.status_message.as_deref(),
                filter_desc: filter_desc.as_deref(),
                selection_count: app.selected_set.len(),
                conversations_mode: app.conversations_mode,
            };
            frame.render_widget(bottom, outer[2]);

            if app.mode == InputMode::FolderPicker {
                let filtered = app.filtered_folders();
                let picker = FolderPicker {
                    folders: &filtered,
                    selected: app.folder_selected,
                    filter: &app.folder_filter,
                    title: "Folders",
                };
                frame.render_widget(picker, size);
            }
            if app.mode == InputMode::MoveToFolder {
                let filtered = app.filtered_folders_plain();
                let picker = FolderPicker {
                    folders: &filtered,
                    selected: app.folder_selected,
                    filter: &app.folder_filter,
                    title: "Move to folder",
                };
                frame.render_widget(picker, size);
            }
            if matches!(app.mode, InputMode::SmartFolderCreate | InputMode::SmartFolderName) {
                let popup = folder_picker::SmartFolderPopup {
                    query: &app.smart_create_query,
                    name: &app.smart_create_name,
                    phase: app.smart_create_phase,
                    preview: &app.smart_create_preview,
                    count: app.smart_create_count,
                    title: if app.creating_split { "New Split" } else { "New Smart Folder" },
                };
                frame.render_widget(popup, size);
            }
            if app.mode == InputMode::MaildirCreate {
                let popup = folder_picker::MaildirCreatePopup {
                    input: &app.maildir_create_input,
                };
                frame.render_widget(popup, size);
            }
            if app.mode == InputMode::CommandPalette {
                let filtered = app.filtered_palette();
                let palette = CommandPalette {
                    entries: &filtered,
                    filter: &app.palette_filter,
                    selected: app.palette_selected,
                };
                frame.render_widget(palette, size);
            }
            if app.mode == InputMode::Help {
                let help = HelpOverlay {
                    scroll: app.help_scroll,
                };
                frame.render_widget(help, size);
            }

            // Account picker dropdown
            if app.mode == InputMode::AccountPicker {
                use ratatui::style::{Color as C, Modifier as M, Style as S};
                use ratatui::widgets::{Clear, Widget as _};

                let anchor_x = app.tab_regions.iter()
                    .find(|r| matches!(r.kind, TabRegionKind::Account))
                    .map(|r| r.x_start)
                    .unwrap_or(0);
                let popup_y = 1u16;
                let accounts = &app.config.accounts;
                let max_name_len = accounts.iter().map(|a| a.name.len()).max().unwrap_or(5);
                let popup_w = (max_name_len + 4) as u16;
                let popup_h = accounts.len() as u16;
                let popup_area = ratatui::layout::Rect::new(
                    anchor_x,
                    popup_y,
                    popup_w.min(size.width.saturating_sub(anchor_x)),
                    popup_h.min(size.height.saturating_sub(popup_y)),
                );

                Clear.render(popup_area, frame.buffer_mut());

                for (i, acct) in accounts.iter().enumerate() {
                    if i as u16 >= popup_area.height {
                        break;
                    }
                    let is_selected = i == app.account_picker_selected;
                    let style = if is_selected {
                        S::default().bg(C::Blue).fg(C::White).add_modifier(M::BOLD)
                    } else {
                        S::default().bg(C::Indexed(236)).fg(C::White)
                    };
                    let label = format!(" {:<width$} ", acct.name, width = max_name_len);
                    let y = popup_area.y + i as u16;
                    frame.buffer_mut().set_string(popup_area.x, y, &label, style);
                    if is_selected {
                        let sel_style = S::default().bg(C::Blue).fg(C::Cyan);
                        frame.buffer_mut().set_string(popup_area.x, y, "\u{25b8}", sel_style);
                    }
                }
            }
        })?;

        if app.should_quit {
            break;
        }

        // Handle compose (requires terminal suspend/resume)
        if let Some(pending) = app.compose_pending.take() {
            let ctx = match pending {
                compose::ComposePending::Ready(ctx) => Some(ctx),
                compose::ComposePending::Kind(kind) => app.build_compose_context(&kind),
            };
            if let Some(ctx) = ctx {
                let from_email = app
                    .account()
                    .map(|a| a.email.as_str())
                    .unwrap_or("user@example.com");

                match compose::build_compose_file(&ctx, from_email) {
                    Ok(content) => {
                        let tmp_path = std::env::temp_dir()
                            .join(format!("hutt-compose-{}.eml", std::process::id()));
                        if std::fs::write(&tmp_path, &content).is_ok() {
                            io::stdout().execute(crossterm::event::DisableMouseCapture)?;
                            terminal::disable_raw_mode()?;
                            io::stdout().execute(LeaveAlternateScreen)?;

                            let modified =
                                compose::launch_editor(&tmp_path, &app.config.editor)
                                    .unwrap_or(false);

                            // Send while terminal is still in normal mode so that
                            // password_command (e.g. pass/gpg pinentry) can use the tty.
                            let send_result = if modified {
                                if let Ok(msg_content) = std::fs::read_to_string(&tmp_path) {
                                    if let Some(acct) = app.account() {
                                        use std::io::Write;
                                        print!("Sending...");
                                        let _ = io::stdout().flush();
                                        match send::send_message(&msg_content, &acct.smtp).await {
                                            Ok(formatted) => {
                                                // Save to Sent maildir
                                                if let Err(e) = save_to_sent(
                                                    &acct.maildir,
                                                    &acct.folders.sent,
                                                    &formatted,
                                                ) {
                                                    println!("\nWarning: sent but failed to save to Sent folder: {}", e);
                                                }
                                                Some(Ok(()))
                                            }
                                            Err(e) => Some(Err(e)),
                                        }
                                    } else {
                                        Some(Err(anyhow::anyhow!("No SMTP account configured")))
                                    }
                                } else {
                                    Some(Err(anyhow::anyhow!("Failed to read compose file")))
                                }
                            } else {
                                None
                            };

                            terminal::enable_raw_mode()?;
                            io::stdout().execute(EnterAlternateScreen)?;
                            io::stdout().execute(crossterm::event::EnableMouseCapture)?;
                            terminal.clear()?;

                            match send_result {
                                Some(Ok(())) => {
                                    app.set_status("Message sent");
                                    app.needs_reindex = true;
                                }
                                Some(Err(e)) => {
                                    app.set_status(format!("Send error: {}", e))
                                }
                                None => app.set_status("Compose cancelled"),
                            }
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                    }
                    Err(e) => app.set_status(format!("Compose error: {}", e)),
                }
            }
            continue;
        }

        // Handle suspended shell command (like compose, needs terminal suspend/resume)
        if let Some(pending) = app.shell_pending.take() {
            io::stdout().execute(crossterm::event::DisableMouseCapture)?;
            terminal::disable_raw_mode()?;
            io::stdout().execute(LeaveAlternateScreen)?;

            let status = std::process::Command::new("sh")
                .args(["-c", &pending.command])
                .status();

            terminal::enable_raw_mode()?;
            io::stdout().execute(EnterAlternateScreen)?;
            io::stdout().execute(crossterm::event::EnableMouseCapture)?;
            terminal.clear()?;

            match status {
                Ok(s) => {
                    debug_log!("shell[{}]: exit={}", pending.command, s);
                    if s.success() {
                        app.set_status(format!("Done: {}", pending.command));
                    } else {
                        app.set_status(format!("Exited {}: {}", s, pending.command));
                    }
                }
                Err(e) => {
                    debug_log!("shell[{}]: error={}", pending.command, e);
                    app.set_status(format!("Failed: {}", e));
                }
            }

            if pending.reindex {
                app.needs_reindex = true;
            }
            continue;
        }

        // Handle key sequence timeout
        if app.keymap.has_pending() && last_key_time.elapsed() > sequence_timeout {
            app.keymap.cancel_pending();
        }

        let timeout = if app.keymap.has_pending() {
            sequence_timeout
        } else {
            Duration::from_millis(100)
        };

        // Start server-side reindex if requested (non-blocking: we poll in the select loop)
        if app.needs_reindex && !app.indexing {
            app.needs_reindex = false;
            debug_log!("reindex: sending (index) to mu server");
            app.set_status("Reindexing...".to_string());
            match app.mu.start_index().await {
                Ok(()) => app.indexing = true,
                Err(e) => {
                    debug_log!("reindex: start_index failed: {}", e);
                    app.set_status(format!("Reindex error: {}", e));
                }
            }
        }

        // Drain any pending IPC commands before blocking on input
        while let Ok(cmd) = ipc_rx.try_recv() {
            debug_log!("IPC drain: {:?}", cmd);
            if let Err(e) = app.handle_ipc_command(cmd).await {
                app.set_status(format!("IPC error: {}", e));
            }
        }

        // Background prefetch: run one queued query per loop iteration
        // during idle time. Each query is fast (~1-100ms) so input lag
        // is imperceptible. Skip if indexing (mu server is busy).
        if !app.prefetch_queue.is_empty() && !app.indexing {
            let item = app.prefetch_queue.remove(0);
            let cache_key = (item.account_idx, item.query.clone());
            if !app.folder_cache.contains_key(&cache_key) {
                // Pick the right mu server for this account
                let mu_server = if item.account_idx == app.active_account {
                    Some(&mut app.mu)
                } else {
                    app.background_mu.get_mut(&item.account_idx)
                };
                if let Some(mu) = mu_server {
                    debug_log!("prefetch: [{}] fetching {:?}",
                        app.config.accounts.get(item.account_idx).map(|a| a.name.as_str()).unwrap_or("?"),
                        item.folder);
                    match mu.find(&item.query, &FindOpts::default()).await {
                        Ok(mut envelopes) => {
                            // Apply split exclusion for active account inbox
                            if item.account_idx == app.active_account {
                                let is_inbox = app.account()
                                    .map(|a| a.folders.inbox.as_str())
                                    .unwrap_or("/Inbox") == item.folder;
                                if is_inbox && !app.split_excluded.is_empty() {
                                    envelopes.retain(|e| !app.split_excluded.contains(&e.docid));
                                }
                            }
                            debug_log!("prefetch: [{}] cached {:?} ({} envelopes)",
                                app.config.accounts.get(item.account_idx).map(|a| a.name.as_str()).unwrap_or("?"),
                                item.folder, envelopes.len());
                            app.folder_cache.insert(cache_key, envelopes);
                        }
                        Err(e) => {
                            debug_log!("prefetch: error for {:?}: {}", item.folder, e);
                        }
                    }
                }
            }
        }

        // Multiplex keyboard events and IPC commands
        let event = tokio::select! {
            ev = event_stream.next() => ev.and_then(|r| r.ok()),
            cmd = ipc_rx.recv() => {
                if let Some(cmd) = cmd {
                    debug_log!("IPC select: {:?}", cmd);
                    if let Err(e) = app.handle_ipc_command(cmd).await {
                        app.set_status(format!("IPC error: {}", e));
                    }
                }
                continue;
            }
            index_frame = app.mu.poll_index_frame(), if app.indexing => {
                match index_frame {
                    Ok(true) => {
                        // Index complete — reload folder
                        app.indexing = false;
                        debug_log!("reindex: complete, reloading folder");
                        app.invalidate_folder_cache();
                        // Refresh split caches before reloading so inbox
                        // exclusions are up to date.
                        app.refresh_split_caches().await;
                        if let Err(e) = app.load_folder().await {
                            debug_log!("reindex: reload error: {}", e);
                        }
                        app.set_status("Reindex complete".to_string());
                    }
                    Ok(false) => {} // progress update, keep polling
                    Err(e) => {
                        app.indexing = false;
                        debug_log!("reindex: error: {}", e);
                        app.set_status(format!("Reindex error: {}", e));
                    }
                }
                continue;
            }
            result = shell_rx.recv() => {
                if let Some(result) = result {
                    match result {
                        Ok(r) => {
                            debug_log!("shell[{}]: exit={}", r.command, r.status);
                            for line in r.stdout.lines() {
                                debug_log!("shell[{}] stdout: {}", r.command, line);
                            }
                            for line in r.stderr.lines() {
                                debug_log!("shell[{}] stderr: {}", r.command, line);
                            }
                            let last_line = r.stderr.lines().last()
                                .or_else(|| r.stdout.lines().last())
                                .unwrap_or("");
                            if r.status.success() {
                                if r.reindex {
                                    app.needs_reindex = true;
                                }
                                if last_line.is_empty() {
                                    app.set_status(format!("Done: {}", r.command));
                                } else {
                                    app.set_status(last_line.to_string());
                                }
                            } else if last_line.is_empty() {
                                app.set_status(format!("Exited {}: {}", r.status, r.command));
                            } else {
                                app.set_status(format!("Exit {}: {}", r.status, last_line));
                            }
                        }
                        Err(e) => {
                            debug_log!("shell[{}]: error={}", e.command, e.error);
                            app.set_status(format!("Failed: {}", e.error));
                        }
                    }
                }
                continue;
            }
            _ = tokio::time::sleep(timeout) => None,
        };

        // Handle mouse events
        if let Some(Event::Mouse(mouse)) = event {
            use crossterm::event::{MouseEventKind, MouseButton};

            // Dismiss account picker on click outside
            if app.mode == InputMode::AccountPicker {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    // Simple: any click dismisses the picker, and if it's on
                    // a valid account row, we switch to it.
                    app.mode = InputMode::Normal;
                }
                continue;
            }

            if app.mode == InputMode::Normal || app.mode == InputMode::Search {
                let size = terminal.size()?;
                let border_col = (size.width as u32 * app.list_pct as u32 / 100) as u16;
                let in_content = mouse.row > 0 && mouse.row < size.height.saturating_sub(1);
                let on_tab_bar = mouse.row == 0;

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if on_tab_bar => {
                        // Check tab bar regions
                        let col = mouse.column;
                        for region in &app.tab_regions {
                            if col >= region.x_start && col < region.x_end {
                                match &region.kind {
                                    TabRegionKind::Account => {
                                        if app.config.accounts.len() > 1 {
                                            app.account_picker_selected = app.active_account;
                                            app.mode = InputMode::AccountPicker;
                                        }
                                    }
                                    TabRegionKind::Tab(i) => {
                                        let folder = app.tabs[*i].clone();
                                        if folder != app.current_folder {
                                            if let Err(e) = app.navigate_folder(&folder).await {
                                                app.set_status(format!("Error: {}", e));
                                            }
                                        }
                                    }
                                    TabRegionKind::Overflow => {
                                        app.folder_filter.clear();
                                        app.folder_selected = 0;
                                        app.mode = InputMode::FolderPicker;
                                    }
                                }
                                break;
                            }
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) if in_content => {
                        // Start drag if clicking within 1 col of the border
                        if mouse.column.abs_diff(border_col) <= 1 {
                            app.dragging_border = true;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) if app.dragging_border => {
                        let new_pct = ((mouse.column as u32) * 100 / (size.width as u32)) as u16;
                        let clamped = new_pct.clamp(10, 90);
                        if clamped != app.list_pct {
                            app.list_pct = clamped;
                            app.preview_cache = RenderCache::new();
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        app.dragging_border = false;
                    }
                    _ => {}
                }
            }
            continue;
        }

        if let Some(Event::Key(key)) = event {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            last_key_time = Instant::now();

            // In popup modes, handle arrow keys for navigation before passing to keymap
            match app.mode {
                InputMode::FolderPicker => {
                    if key.code == crossterm::event::KeyCode::Down {
                        let max = app.filtered_folders().len();
                        if app.folder_selected + 1 < max {
                            app.folder_selected += 1;
                        }
                        continue;
                    }
                    if key.code == crossterm::event::KeyCode::Up {
                        app.folder_selected = app.folder_selected.saturating_sub(1);
                        continue;
                    }
                    // Ctrl-D deletes the selected folder
                    if key.code == crossterm::event::KeyCode::Char('d')
                        && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        app.delete_selected_folder().await;
                        continue;
                    }
                }
                InputMode::MoveToFolder => {
                    if key.code == crossterm::event::KeyCode::Down {
                        let max = app.filtered_folders_plain().len();
                        if app.folder_selected + 1 < max {
                            app.folder_selected += 1;
                        }
                        continue;
                    }
                    if key.code == crossterm::event::KeyCode::Up {
                        app.folder_selected = app.folder_selected.saturating_sub(1);
                        continue;
                    }
                }
                InputMode::SmartFolderCreate | InputMode::SmartFolderName | InputMode::MaildirCreate => {
                    // These modes use text input only, no arrow key navigation
                }
                InputMode::CommandPalette => {
                    if key.code == crossterm::event::KeyCode::Down {
                        let max = app.filtered_palette().len();
                        if app.palette_selected + 1 < max {
                            app.palette_selected += 1;
                        }
                        continue;
                    }
                    if key.code == crossterm::event::KeyCode::Up {
                        app.palette_selected = app.palette_selected.saturating_sub(1);
                        continue;
                    }
                }
                InputMode::AccountPicker => {
                    match key.code {
                        crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                            if app.account_picker_selected + 1 < app.config.accounts.len() {
                                app.account_picker_selected += 1;
                            }
                            continue;
                        }
                        crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                            app.account_picker_selected = app.account_picker_selected.saturating_sub(1);
                            continue;
                        }
                        crossterm::event::KeyCode::Enter => {
                            let idx = app.account_picker_selected;
                            app.mode = InputMode::Normal;
                            if idx != app.active_account {
                                if let Err(e) = app.switch_account(idx).await {
                                    app.set_status(format!("Account switch error: {}", e));
                                }
                            }
                            continue;
                        }
                        crossterm::event::KeyCode::Esc => {
                            app.mode = InputMode::Normal;
                            continue;
                        }
                        _ => { continue; }
                    }
                }
                _ => {}
            }

            let action = app.keymap.handle(key, &app.mode);
            if action == Action::Redraw {
                terminal.clear()?;
                continue;
            }
            if let Err(e) = app.handle_action(action).await {
                app.set_status(format!("Error: {}", e));
            }
        }
    }

    io::stdout().execute(crossterm::event::DisableMouseCapture)?;
    terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    for (_, mut bg) in app.background_mu.drain() {
        let _ = bg.quit().await;
    }
    app.mu.quit().await?;
    Ok(())
}
