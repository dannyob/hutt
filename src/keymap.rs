use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    MoveDown,
    MoveUp,
    JumpTop,
    JumpBottom,
    ScrollPreviewDown,
    ScrollPreviewUp,
    Quit,
    // Placeholder for future phases
    Noop,
}

/// Tracks multi-key sequences (e.g., g then g for JumpTop).
pub struct KeyMapper {
    pending: Option<KeyCode>,
}

impl KeyMapper {
    pub fn new() -> Self {
        Self { pending: None }
    }

    /// Process a key event and return an action (or Noop).
    pub fn handle(&mut self, key: KeyEvent) -> Action {
        // If we have a pending first key of a sequence
        if let Some(first) = self.pending.take() {
            return self.handle_sequence(first, key);
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => Action::MoveDown,
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => Action::MoveUp,
            (KeyCode::Char('g'), KeyModifiers::NONE) => {
                // Start of a potential 'gg' sequence
                self.pending = Some(KeyCode::Char('g'));
                Action::Noop
            }
            (KeyCode::Char('G'), KeyModifiers::SHIFT) => Action::JumpBottom,
            (KeyCode::Char(' '), KeyModifiers::NONE) => Action::ScrollPreviewDown,
            (KeyCode::Char(' '), KeyModifiers::SHIFT) => Action::ScrollPreviewUp,
            (KeyCode::Char('q'), KeyModifiers::NONE) => Action::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,
            _ => Action::Noop,
        }
    }

    fn handle_sequence(&mut self, first: KeyCode, key: KeyEvent) -> Action {
        match (first, key.code) {
            // gg -> jump to top
            (KeyCode::Char('g'), KeyCode::Char('g')) => Action::JumpTop,
            // Any other key after g -> treat as noop for now
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
}
