use chrono::{DateTime, Utc};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{}", name),
            None => write!(f, "{}", self.email),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flag {
    Seen,
    Replied,
    Flagged,
    Trashed,
    Draft,
    Passed,
    List,
    Unread,
}

impl Flag {
    pub fn from_symbol(s: &str) -> Option<Self> {
        match s {
            "seen" => Some(Flag::Seen),
            "replied" => Some(Flag::Replied),
            "flagged" => Some(Flag::Flagged),
            "trashed" => Some(Flag::Trashed),
            "draft" => Some(Flag::Draft),
            "passed" => Some(Flag::Passed),
            "list" => Some(Flag::List),
            "unread" => Some(Flag::Unread),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThreadMeta {
    pub level: u32,
    pub root: bool,
    pub thread_subject: bool,
}

impl Default for ThreadMeta {
    fn default() -> Self {
        Self {
            level: 0,
            root: true,
            thread_subject: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub docid: u32,
    pub message_id: String,
    pub subject: String,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub date: DateTime<Utc>,
    pub flags: Vec<Flag>,
    pub maildir: String,
    pub path: PathBuf,
    pub thread_meta: ThreadMeta,
}

impl Envelope {
    pub fn is_unread(&self) -> bool {
        !self.flags.contains(&Flag::Seen)
    }

    pub fn is_flagged(&self) -> bool {
        self.flags.contains(&Flag::Flagged)
    }

    pub fn from_display(&self) -> String {
        self.from
            .first()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "(unknown)".to_string())
    }

    pub fn date_display(&self) -> String {
        let now = Utc::now();
        let date = self.date;
        if now.date_naive() == date.date_naive() {
            date.format("%H:%M").to_string()
        } else if (now - date).num_days() < 7 {
            date.format("%a %H:%M").to_string()
        } else if now.format("%Y").to_string() == date.format("%Y").to_string() {
            date.format("%b %d").to_string()
        } else {
            date.format("%Y-%m-%d").to_string()
        }
    }
}
