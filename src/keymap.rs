use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

use crate::config::{BindingValue, BindingsSection};

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    ThreadView,
    FolderPicker,
    CommandPalette,
    Help,
    SmartFolderCreate,
    SmartFolderName,
    MaildirCreate,
    MoveToFolder,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Navigation
    MoveDown,
    MoveUp,
    JumpTop,
    JumpBottom,
    ScrollPreviewDown,
    ScrollPreviewUp,
    HalfPageDown,
    HalfPageUp,
    FullPageDown,
    FullPageUp,

    // Triage — MoveToFolder(None) opens picker, Some("archive") resolves
    // from account folders config, Some("/Literal") uses path directly.
    MoveToFolder(Option<String>),
    ToggleRead,
    ToggleStar,
    Undo,

    // Folder switching (g-prefix sequences)
    GoInbox,
    GoArchive,
    GoDrafts,
    GoSent,
    GoTrash,
    GoSpam,
    GoFolderPicker,

    // Folder cycling
    NextFolder,
    PrevFolder,

    // Account switching
    NextAccount,
    PrevAccount,

    // Search & Filters
    EnterSearch,
    FilterUnread,
    FilterStarred,
    FilterNeedsReply,

    // Multi-select
    ToggleSelect,
    SelectDown,
    SelectUp,

    // Thread view
    OpenThread,
    CloseThread,
    ThreadNext,
    ThreadPrev,
    ThreadToggleExpand,
    ThreadExpandAll,

    // Compose (Phase 2)
    Compose,
    Reply,
    ReplyAll,
    Forward,

    // Linkability (Phase 3)
    CopyMessageUrl,
    CopyThreadUrl,
    OpenInBrowser,

    // Command palette (Phase 4)
    OpenCommandPalette,

    // Conversations
    ToggleConversations,

    // Help
    ShowHelp,

    // Sync (Phase 4)
    SyncMail,

    // Custom bindings
    RunShell {
        command: String,
        reindex: bool,
        suspend: bool,
    },
    NavigateFolder(String),

    // Text input (shared across input modes)
    InputChar(char),
    InputBackspace,
    InputSubmit,
    InputCancel,
    InputHistoryPrev,
    InputHistoryNext,

    // System
    Redraw,
    Quit,
    Noop,
}

// ---------------------------------------------------------------------------
// Key parsing — converts config strings to crossterm types
// ---------------------------------------------------------------------------

/// A single key press (code + modifiers), comparable and hashable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}


/// A full key trigger: either a single press or a two-key sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyTrigger {
    Single(KeyCombo),
    Sequence(KeyCombo, KeyCombo),
}

/// What a custom binding resolves to at runtime.
#[derive(Debug, Clone)]
pub enum BindAction {
    Builtin(Action),
    Shell {
        command: String,
        reindex: bool,
        suspend: bool,
    },
    Folder(String),
}

/// A fully parsed binding ready for lookup.
#[derive(Debug, Clone)]
pub struct Binding {
    pub trigger: KeyTrigger,
    pub action: BindAction,
    pub modes: Vec<InputMode>,
}

/// Parse a key string like `"ctrl+r"`, `"G"`, `"g i"` into a `KeyTrigger`.
pub fn parse_key_string(s: &str) -> Result<KeyTrigger, String> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.len() {
        1 => Ok(KeyTrigger::Single(parse_key_combo(parts[0])?)),
        2 => Ok(KeyTrigger::Sequence(
            parse_key_combo(parts[0])?,
            parse_key_combo(parts[1])?,
        )),
        _ => Err(format!("key {:?}: at most 2 keys in a sequence", s)),
    }
}

/// Parse a single key combo like `"ctrl+r"`, `"G"`, `"#"`, `"enter"`.
fn parse_key_combo(s: &str) -> Result<KeyCombo, String> {
    let lower = s.to_lowercase();

    if let Some(rest) = lower.strip_prefix("ctrl+") {
        let code = parse_key_name(rest)?;
        return Ok(KeyCombo {
            code,
            modifiers: KeyModifiers::CONTROL,
        });
    }
    if let Some(rest) = lower.strip_prefix("shift+") {
        let code = parse_key_name(rest)?;
        return Ok(KeyCombo {
            code,
            modifiers: KeyModifiers::SHIFT,
        });
    }

    // Single character
    if s.len() == 1 {
        let c = s.chars().next().unwrap();
        if c.is_ascii_uppercase() {
            return Ok(KeyCombo {
                code: KeyCode::Char(c),
                modifiers: KeyModifiers::SHIFT,
            });
        }
        return Ok(KeyCombo {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        });
    }

    // Named key
    let code = parse_key_name(&lower)?;
    Ok(KeyCombo {
        code,
        modifiers: KeyModifiers::NONE,
    })
}

fn parse_key_name(name: &str) -> Result<KeyCode, String> {
    match name {
        "enter" | "return" => Ok(KeyCode::Enter),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "space" => Ok(KeyCode::Char(' ')),
        "tab" => Ok(KeyCode::Tab),
        "backspace" => Ok(KeyCode::Backspace),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        s if s.len() == 1 => Ok(KeyCode::Char(s.chars().next().unwrap())),
        s if s.starts_with('f') => s[1..]
            .parse::<u8>()
            .map(KeyCode::F)
            .map_err(|_| format!("unknown key: {:?}", s)),
        _ => Err(format!("unknown key: {:?}", name)),
    }
}

/// Parse a built-in action name from config.
pub fn parse_action_name(name: &str) -> Result<Action, String> {
    match name {
        "move_down" => Ok(Action::MoveDown),
        "move_up" => Ok(Action::MoveUp),
        "jump_top" => Ok(Action::JumpTop),
        "jump_bottom" => Ok(Action::JumpBottom),
        "scroll_preview_down" => Ok(Action::ScrollPreviewDown),
        "scroll_preview_up" => Ok(Action::ScrollPreviewUp),
        "half_page_down" => Ok(Action::HalfPageDown),
        "half_page_up" => Ok(Action::HalfPageUp),
        "full_page_down" => Ok(Action::FullPageDown),
        "full_page_up" => Ok(Action::FullPageUp),
        "archive" => Ok(Action::MoveToFolder(Some("archive".to_string()))),
        "trash" => Ok(Action::MoveToFolder(Some("trash".to_string()))),
        "spam" => Ok(Action::MoveToFolder(Some("spam".to_string()))),
        "move_to_folder" | "move" => Ok(Action::MoveToFolder(None)),
        "toggle_read" => Ok(Action::ToggleRead),
        "toggle_star" => Ok(Action::ToggleStar),
        "undo" => Ok(Action::Undo),
        "go_inbox" => Ok(Action::GoInbox),
        "go_archive" => Ok(Action::GoArchive),
        "go_drafts" => Ok(Action::GoDrafts),
        "go_sent" => Ok(Action::GoSent),
        "go_trash" => Ok(Action::GoTrash),
        "go_spam" => Ok(Action::GoSpam),
        "go_folder_picker" => Ok(Action::GoFolderPicker),
        "next_folder" => Ok(Action::NextFolder),
        "prev_folder" => Ok(Action::PrevFolder),
        "next_account" => Ok(Action::NextAccount),
        "prev_account" => Ok(Action::PrevAccount),
        "enter_search" | "search" => Ok(Action::EnterSearch),
        "filter_unread" => Ok(Action::FilterUnread),
        "filter_starred" => Ok(Action::FilterStarred),
        "filter_needs_reply" => Ok(Action::FilterNeedsReply),
        "toggle_select" => Ok(Action::ToggleSelect),
        "select_down" => Ok(Action::SelectDown),
        "select_up" => Ok(Action::SelectUp),
        "open_thread" => Ok(Action::OpenThread),
        "close_thread" => Ok(Action::CloseThread),
        "thread_next" => Ok(Action::ThreadNext),
        "thread_prev" => Ok(Action::ThreadPrev),
        "thread_toggle_expand" => Ok(Action::ThreadToggleExpand),
        "thread_expand_all" => Ok(Action::ThreadExpandAll),
        "compose" => Ok(Action::Compose),
        "reply" => Ok(Action::Reply),
        "reply_all" => Ok(Action::ReplyAll),
        "forward" => Ok(Action::Forward),
        "copy_message_url" => Ok(Action::CopyMessageUrl),
        "copy_thread_url" => Ok(Action::CopyThreadUrl),
        "open_in_browser" => Ok(Action::OpenInBrowser),
        "open_command_palette" | "command_palette" => Ok(Action::OpenCommandPalette),
        "toggle_conversations" | "conversations" => Ok(Action::ToggleConversations),
        "show_help" | "help" => Ok(Action::ShowHelp),
        "sync_mail" | "sync" => Ok(Action::SyncMail),
        "quit" => Ok(Action::Quit),
        _ => Err(format!("unknown action: {:?}", name)),
    }
}

#[allow(dead_code)] // reserved for future per-mode config in [bindings.*]
fn parse_mode_name(name: &str) -> Result<InputMode, String> {
    match name {
        "normal" => Ok(InputMode::Normal),
        "thread" | "thread_view" => Ok(InputMode::ThreadView),
        _ => Err(format!("unknown mode: {:?}", name)),
    }
}

/// Convert a `BindingValue` from config into a `BindAction`.
fn resolve_binding_value(value: &BindingValue) -> Result<BindAction, String> {
    match value {
        BindingValue::Short(s) => {
            if s.starts_with('/') {
                Ok(BindAction::Folder(s.clone()))
            } else {
                Ok(BindAction::Builtin(parse_action_name(s)?))
            }
        }
        BindingValue::Shell {
            shell,
            reindex,
            suspend,
        } => Ok(BindAction::Shell {
            command: shell.clone(),
            reindex: *reindex,
            suspend: *suspend,
        }),
        BindingValue::Move { folder } => {
            Ok(BindAction::Builtin(Action::MoveToFolder(Some(folder.clone()))))
        }
    }
}

// ---------------------------------------------------------------------------
// KeyMapper
// ---------------------------------------------------------------------------

/// Tracks multi-key sequences (e.g., g then g for JumpTop, g then i for GoInbox)
/// and custom keybindings from config.
pub struct KeyMapper {
    pending: Option<KeyCode>,
    /// Custom bindings from config, checked before hardcoded defaults.
    custom_bindings: Vec<Binding>,
    /// First keys of custom two-key sequences — when pressed, enter pending state.
    custom_prefixes: HashSet<KeyCombo>,
}

impl KeyMapper {
    pub fn new() -> Self {
        Self {
            pending: None,
            custom_bindings: Vec::new(),
            custom_prefixes: HashSet::new(),
        }
    }

    /// Load custom bindings from config.  Invalid entries are logged and skipped.
    pub fn load_bindings(&mut self, section: &BindingsSection) {
        self.custom_bindings.clear();
        self.custom_prefixes.clear();

        let scopes: &[(&std::collections::HashMap<String, BindingValue>, Vec<InputMode>)] = &[
            (
                &section.global,
                vec![InputMode::Normal, InputMode::ThreadView],
            ),
            (&section.normal, vec![InputMode::Normal]),
            (&section.thread, vec![InputMode::ThreadView]),
        ];

        for (map, modes) in scopes {
            for (key_str, value) in *map {
                match self.parse_binding(key_str, value, modes.clone()) {
                    Ok(binding) => {
                        if let KeyTrigger::Sequence(ref first, _) = binding.trigger {
                            self.custom_prefixes.insert(first.clone());
                        }
                        self.custom_bindings.push(binding);
                    }
                    Err(e) => {
                        eprintln!("hutt: ignoring invalid binding {:?}: {}", key_str, e);
                    }
                }
            }
        }
    }

    fn parse_binding(
        &self,
        key_str: &str,
        value: &BindingValue,
        modes: Vec<InputMode>,
    ) -> Result<Binding, String> {
        let trigger = parse_key_string(key_str)?;
        let action = resolve_binding_value(value)?;
        Ok(Binding {
            trigger,
            action,
            modes,
        })
    }

    /// Look up a trigger in custom bindings for the given mode.
    fn lookup_custom(&self, trigger: &KeyTrigger, mode: &InputMode) -> Option<Action> {
        for binding in &self.custom_bindings {
            if !binding.modes.contains(mode) {
                continue;
            }
            let matched = match (&binding.trigger, trigger) {
                (KeyTrigger::Single(a), KeyTrigger::Single(b)) => a == b,
                (KeyTrigger::Sequence(a1, a2), KeyTrigger::Sequence(b1, b2)) => {
                    a1 == b1 && a2 == b2
                }
                _ => false,
            };
            if matched {
                return Some(match &binding.action {
                    BindAction::Builtin(a) => a.clone(),
                    BindAction::Shell {
                        command,
                        reindex,
                        suspend,
                    } => Action::RunShell {
                        command: command.clone(),
                        reindex: *reindex,
                        suspend: *suspend,
                    },
                    BindAction::Folder(path) => Action::NavigateFolder(path.clone()),
                });
            }
        }
        None
    }

    /// Process a key event and return an action, considering current input mode.
    pub fn handle(&mut self, key: KeyEvent, mode: &InputMode) -> Action {
        // Input modes never use custom bindings (they need raw chars)
        match mode {
            InputMode::Search
            | InputMode::FolderPicker
            | InputMode::MoveToFolder
            | InputMode::CommandPalette
            | InputMode::SmartFolderCreate
            | InputMode::SmartFolderName
            | InputMode::MaildirCreate => {
                return self.handle_input(key);
            }
            _ => {}
        }

        // If we have a pending first key, check custom sequences first
        if let Some(first_code) = self.pending.take() {
            let first_combo = KeyCombo {
                code: first_code,
                modifiers: KeyModifiers::NONE,
            };
            let second_combo = KeyCombo {
                code: key.code,
                modifiers: key.modifiers,
            };
            let trigger = KeyTrigger::Sequence(first_combo, second_combo);
            if let Some(action) = self.lookup_custom(&trigger, mode) {
                return action;
            }
            // Fall through to hardcoded sequences
            return self.handle_sequence(first_code, key);
        }

        // Check custom single-key bindings
        let combo = KeyCombo {
            code: key.code,
            modifiers: key.modifiers,
        };
        if let Some(action) = self.lookup_custom(&KeyTrigger::Single(combo.clone()), mode) {
            return action;
        }

        // Check if this key starts a custom sequence
        if self.custom_prefixes.contains(&combo) {
            self.pending = Some(key.code);
            return Action::Noop;
        }

        // Fall through to hardcoded handlers
        match mode {
            InputMode::Normal => self.handle_normal(key),
            InputMode::ThreadView => self.handle_thread(key),
            InputMode::Help => self.handle_help(key),
            _ => Action::Noop,
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Action {
        // If we have a pending first key of a sequence
        if let Some(first) = self.pending.take() {
            return self.handle_sequence(first, key);
        }

        match (key.code, key.modifiers) {
            // Navigation
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => Action::MoveDown,
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => Action::MoveUp,
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                self.pending = Some(KeyCode::Char('g'));
                Action::Noop
            }
            (KeyCode::Char('G'), KeyModifiers::SHIFT) => Action::JumpBottom,
            (KeyCode::Char(' '), KeyModifiers::NONE) => Action::ScrollPreviewDown,
            (KeyCode::Char(' '), KeyModifiers::SHIFT) => Action::ScrollPreviewUp,
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => Action::HalfPageDown,
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => Action::HalfPageUp,
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => Action::FullPageDown,
            (KeyCode::Char('b'), KeyModifiers::CONTROL) => Action::FullPageUp,

            // Triage
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                Action::MoveToFolder(Some("archive".to_string()))
            }
            (KeyCode::Char('#'), _) => Action::MoveToFolder(Some("trash".to_string())),
            (KeyCode::Char('!'), _) => Action::MoveToFolder(Some("spam".to_string())),
            (KeyCode::Char('m'), KeyModifiers::NONE) => Action::MoveToFolder(None),
            // Note: 'u' without Ctrl is ToggleRead
            (KeyCode::Char('u'), KeyModifiers::NONE) => Action::ToggleRead,
            (KeyCode::Char('s'), KeyModifiers::NONE) => Action::ToggleStar,
            (KeyCode::Char('z'), KeyModifiers::NONE) => Action::Undo,

            // Multi-select
            (KeyCode::Char('x'), KeyModifiers::NONE) => Action::ToggleSelect,
            (KeyCode::Char('J'), KeyModifiers::SHIFT) => Action::SelectDown,
            (KeyCode::Char('K'), KeyModifiers::SHIFT) => Action::SelectUp,

            // Search & Filters
            (KeyCode::Char('/'), _) => Action::EnterSearch,
            (KeyCode::Char('U'), KeyModifiers::SHIFT) => Action::FilterUnread,
            (KeyCode::Char('S'), KeyModifiers::SHIFT) => Action::FilterStarred,
            (KeyCode::Char('R'), KeyModifiers::SHIFT) => Action::FilterNeedsReply,

            // Thread view
            (KeyCode::Enter, _) => Action::OpenThread,

            // Compose
            (KeyCode::Char('c'), KeyModifiers::NONE) => Action::Compose,
            (KeyCode::Char('r'), KeyModifiers::NONE) => Action::Reply,
            (KeyCode::Char('a'), KeyModifiers::NONE) => Action::ReplyAll,
            (KeyCode::Char('f'), KeyModifiers::NONE) => Action::Forward,

            // Linkability
            (KeyCode::Char('y'), KeyModifiers::NONE) => Action::CopyMessageUrl,
            (KeyCode::Char('Y'), KeyModifiers::SHIFT) => Action::CopyThreadUrl,
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => Action::OpenInBrowser,

            // Command palette
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => Action::OpenCommandPalette,

            // Sync
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => Action::SyncMail,

            // Redraw
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => Action::Redraw,

            // Conversations
            (KeyCode::Char('V'), KeyModifiers::SHIFT) => Action::ToggleConversations,

            // Help
            (KeyCode::Char('?'), _) => Action::ShowHelp,

            // Folder cycling
            (KeyCode::Tab, _) => Action::NextFolder,
            (KeyCode::BackTab, _) => Action::PrevFolder,

            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => Action::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,

            _ => Action::Noop,
        }
    }

    fn handle_sequence(&mut self, first: KeyCode, key: KeyEvent) -> Action {
        match (first, key.code) {
            // gg -> jump to top
            (KeyCode::Char('g'), KeyCode::Char('g')) => Action::JumpTop,
            // g-prefix folder switching
            (KeyCode::Char('g'), KeyCode::Char('i')) => Action::GoInbox,
            (KeyCode::Char('g'), KeyCode::Char('a')) => Action::GoArchive,
            (KeyCode::Char('g'), KeyCode::Char('d')) => Action::GoDrafts,
            (KeyCode::Char('g'), KeyCode::Char('t')) => Action::GoSent,
            (KeyCode::Char('g'), KeyCode::Char('#')) => Action::GoTrash,
            (KeyCode::Char('g'), KeyCode::Char('!')) => Action::GoSpam,
            (KeyCode::Char('g'), KeyCode::Char('l')) => Action::GoFolderPicker,
            // g-prefix account switching
            (KeyCode::Char('g'), KeyCode::Tab) => Action::NextAccount,
            (KeyCode::Char('g'), KeyCode::BackTab) => Action::PrevAccount,
            _ => Action::Noop,
        }
    }

    fn handle_input(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::InputCancel,
            KeyCode::Enter => Action::InputSubmit,
            KeyCode::Backspace => Action::InputBackspace,
            KeyCode::Up => Action::InputHistoryPrev,
            KeyCode::Down => Action::InputHistoryNext,
            KeyCode::Char(c) => {
                // Allow Ctrl+C to quit even in input mode
                if c == 'c' && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Action::Quit;
                }
                Action::InputChar(c)
            }
            _ => Action::Noop,
        }
    }

    fn handle_thread(&mut self, key: KeyEvent) -> Action {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => Action::CloseThread,
            (KeyCode::Char('j'), KeyModifiers::NONE)
            | (KeyCode::Char('n'), KeyModifiers::NONE)
            | (KeyCode::Down, _) => Action::ThreadNext,
            (KeyCode::Char('k'), KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::NONE)
            | (KeyCode::Up, _) => Action::ThreadPrev,
            (KeyCode::Char('o'), KeyModifiers::NONE) | (KeyCode::Enter, _) => {
                Action::ThreadToggleExpand
            }
            (KeyCode::Char('O'), KeyModifiers::SHIFT) => Action::ThreadExpandAll,
            (KeyCode::Char(' '), KeyModifiers::NONE) => Action::ScrollPreviewDown,
            (KeyCode::Char(' '), KeyModifiers::SHIFT) => Action::ScrollPreviewUp,
            // Triage actions still work in thread view
            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                Action::MoveToFolder(Some("archive".to_string()))
            }
            (KeyCode::Char('#'), _) => Action::MoveToFolder(Some("trash".to_string())),
            (KeyCode::Char('!'), _) => Action::MoveToFolder(Some("spam".to_string())),
            (KeyCode::Char('m'), KeyModifiers::NONE) => Action::MoveToFolder(None),
            (KeyCode::Char('u'), KeyModifiers::NONE) => Action::ToggleRead,
            (KeyCode::Char('s'), KeyModifiers::NONE) => Action::ToggleStar,
            (KeyCode::Char('z'), KeyModifiers::NONE) => Action::Undo,
            // Compose from thread view
            (KeyCode::Char('r'), KeyModifiers::NONE) => Action::Reply,
            (KeyCode::Char('a'), KeyModifiers::NONE) => Action::ReplyAll,
            (KeyCode::Char('f'), KeyModifiers::NONE) => Action::Forward,
            // Folder cycling
            (KeyCode::Tab, _) => Action::NextFolder,
            (KeyCode::BackTab, _) => Action::PrevFolder,
            // Help
            (KeyCode::Char('?'), _) => Action::ShowHelp,
            // Quit
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,
            _ => Action::Noop,
        }
    }

    fn handle_help(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => Action::InputCancel,
            KeyCode::Char('j') | KeyCode::Down => Action::ScrollPreviewDown,
            KeyCode::Char('k') | KeyCode::Up => Action::ScrollPreviewUp,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
            _ => Action::Noop,
        }
    }

    /// Cancel any pending sequence (e.g., on timeout).
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn pending_display(&self) -> Option<String> {
        self.pending.map(|code| match code {
            KeyCode::Char(c) => c.to_string(),
            _ => "...".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_char() {
        assert_eq!(
            parse_key_string("e").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Char('e'),
                modifiers: KeyModifiers::NONE,
            })
        );
    }

    #[test]
    fn parse_uppercase_as_shift() {
        assert_eq!(
            parse_key_string("G").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Char('G'),
                modifiers: KeyModifiers::SHIFT,
            })
        );
    }

    #[test]
    fn parse_symbol_char() {
        assert_eq!(
            parse_key_string("#").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Char('#'),
                modifiers: KeyModifiers::NONE,
            })
        );
    }

    #[test]
    fn parse_ctrl_combo() {
        assert_eq!(
            parse_key_string("ctrl+r").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::CONTROL,
            })
        );
    }

    #[test]
    fn parse_shift_space() {
        assert_eq!(
            parse_key_string("shift+space").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Char(' '),
                modifiers: KeyModifiers::SHIFT,
            })
        );
    }

    #[test]
    fn parse_sequence() {
        assert_eq!(
            parse_key_string("g i").unwrap(),
            KeyTrigger::Sequence(
                KeyCombo {
                    code: KeyCode::Char('g'),
                    modifiers: KeyModifiers::NONE,
                },
                KeyCombo {
                    code: KeyCode::Char('i'),
                    modifiers: KeyModifiers::NONE,
                },
            )
        );
    }

    #[test]
    fn parse_special_key() {
        assert_eq!(
            parse_key_string("enter").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
            })
        );
    }

    #[test]
    fn parse_f_key() {
        assert_eq!(
            parse_key_string("f5").unwrap(),
            KeyTrigger::Single(KeyCombo {
                code: KeyCode::F(5),
                modifiers: KeyModifiers::NONE,
            })
        );
    }

    #[test]
    fn reject_triple_sequence() {
        assert!(parse_key_string("a b c").is_err());
    }

    #[test]
    fn action_name_roundtrip() {
        let names = [
            "archive",
            "trash",
            "spam",
            "move_to_folder",
            "move_down",
            "sync_mail",
            "quit",
            "open_thread",
            "compose",
            "reply_all",
            "help",
        ];
        for name in &names {
            assert!(
                parse_action_name(name).is_ok(),
                "failed to parse {:?}",
                name
            );
        }
    }

    #[test]
    fn unknown_action_name() {
        assert!(parse_action_name("bogus").is_err());
    }

    #[test]
    fn custom_binding_overrides_default() {
        let section = BindingsSection {
            global: [("e".to_string(), BindingValue::Short("trash".to_string()))]
                .into_iter()
                .collect(),
            normal: Default::default(),
            thread: Default::default(),
        };
        let mut mapper = KeyMapper::new();
        mapper.load_bindings(&section);

        let key = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE);
        let action = mapper.handle(key, &InputMode::Normal);
        assert_eq!(action, Action::MoveToFolder(Some("trash".to_string()))); // overridden from archive
    }

    #[test]
    fn custom_shell_binding() {
        let section = BindingsSection {
            global: [(
                "G".to_string(),
                BindingValue::Shell {
                    shell: "mbsync almnck".to_string(),
                    reindex: true,
                    suspend: false,
                },
            )]
            .into_iter()
            .collect(),
            normal: Default::default(),
            thread: Default::default(),
        };
        let mut mapper = KeyMapper::new();
        mapper.load_bindings(&section);

        let key = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        let action = mapper.handle(key, &InputMode::Normal);
        assert_eq!(
            action,
            Action::RunShell {
                command: "mbsync almnck".to_string(),
                reindex: true,
                suspend: false,
            }
        );
    }

    #[test]
    fn custom_folder_binding() {
        let section = BindingsSection {
            global: [(
                "g s".to_string(),
                BindingValue::Short("/Sent".to_string()),
            )]
            .into_iter()
            .collect(),
            normal: Default::default(),
            thread: Default::default(),
        };
        let mut mapper = KeyMapper::new();
        mapper.load_bindings(&section);

        // Press 'g' — should enter pending state
        let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
        let action = mapper.handle(g, &InputMode::Normal);
        assert_eq!(action, Action::Noop);

        // Press 's' — should resolve to NavigateFolder
        let s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        let action = mapper.handle(s, &InputMode::Normal);
        assert_eq!(action, Action::NavigateFolder("/Sent".to_string()));
    }

    #[test]
    fn per_mode_binding() {
        let section = BindingsSection {
            global: Default::default(),
            normal: [("o".to_string(), BindingValue::Short("open_thread".to_string()))]
                .into_iter()
                .collect(),
            thread: [(
                "o".to_string(),
                BindingValue::Short("thread_toggle_expand".to_string()),
            )]
            .into_iter()
            .collect(),
        };
        let mut mapper = KeyMapper::new();
        mapper.load_bindings(&section);

        let o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE);
        assert_eq!(
            mapper.handle(o, &InputMode::Normal),
            Action::OpenThread
        );
        let o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE);
        assert_eq!(
            mapper.handle(o, &InputMode::ThreadView),
            Action::ThreadToggleExpand
        );
    }
}
