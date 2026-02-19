use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    ThreadView,
    FolderPicker,
    CommandPalette,
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

    // Triage
    Archive,
    Trash,
    Spam,
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

    // Sync (Phase 4)
    SyncMail,

    // Text input (shared across input modes)
    InputChar(char),
    InputBackspace,
    InputSubmit,
    InputCancel,

    // System
    Quit,
    Noop,
}

/// Tracks multi-key sequences (e.g., g then g for JumpTop, g then i for GoInbox).
pub struct KeyMapper {
    pending: Option<KeyCode>,
}

impl KeyMapper {
    pub fn new() -> Self {
        Self { pending: None }
    }

    /// Process a key event and return an action, considering current input mode.
    pub fn handle(&mut self, key: KeyEvent, mode: &InputMode) -> Action {
        match mode {
            InputMode::Normal => self.handle_normal(key),
            InputMode::Search | InputMode::FolderPicker | InputMode::CommandPalette => {
                self.handle_input(key)
            }
            InputMode::ThreadView => self.handle_thread(key),
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

            // Triage
            (KeyCode::Char('e'), KeyModifiers::NONE) => Action::Archive,
            (KeyCode::Char('#'), _) => Action::Trash,
            (KeyCode::Char('!'), _) => Action::Spam,
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
            _ => Action::Noop,
        }
    }

    fn handle_input(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::InputCancel,
            KeyCode::Enter => Action::InputSubmit,
            KeyCode::Backspace => Action::InputBackspace,
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
            (KeyCode::Char('e'), KeyModifiers::NONE) => Action::Archive,
            (KeyCode::Char('#'), _) => Action::Trash,
            (KeyCode::Char('!'), _) => Action::Spam,
            (KeyCode::Char('u'), KeyModifiers::NONE) => Action::ToggleRead,
            (KeyCode::Char('s'), KeyModifiers::NONE) => Action::ToggleStar,
            (KeyCode::Char('z'), KeyModifiers::NONE) => Action::Undo,
            // Compose from thread view
            (KeyCode::Char('r'), KeyModifiers::NONE) => Action::Reply,
            (KeyCode::Char('a'), KeyModifiers::NONE) => Action::ReplyAll,
            (KeyCode::Char('f'), KeyModifiers::NONE) => Action::Forward,
            // Quit
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,
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

    pub fn pending_display(&self) -> Option<&str> {
        match self.pending {
            Some(KeyCode::Char('g')) => Some("g"),
            _ => None,
        }
    }
}
