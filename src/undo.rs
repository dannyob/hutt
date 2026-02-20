//! Undo stack for triage and folder actions.
//! Each entry records the state before an action so it can be reversed.

use crate::smart_folders::SmartFolder;

pub enum UndoAction {
    MoveMessage {
        docid: u32,
        original_maildir: String,
        original_flags: String,
    },
    DeleteSmartFolder {
        folder: SmartFolder,
    },
    DeleteMaildirFolder {
        path: String,
    },
}

pub struct UndoEntry {
    pub action: UndoAction,
    pub description: String,
}

pub struct UndoStack {
    entries: Vec<UndoEntry>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, entry: UndoEntry) {
        self.entries.push(entry);
    }

    pub fn pop(&mut self) -> Option<UndoEntry> {
        self.entries.pop()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
