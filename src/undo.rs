/// Undo stack for triage actions.
/// Each entry records the state before a triage action so it can be reversed.
pub struct UndoEntry {
    pub docid: u32,
    pub original_maildir: String,
    pub original_flags: String,
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

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
