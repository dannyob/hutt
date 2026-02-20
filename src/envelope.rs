use chrono::{DateTime, Utc};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl Address {
    /// Name only (for compact list views), falls back to email.
    pub fn short_display(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.email.clone())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{} <{}>", name, self.email),
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

    /// Convert a mu single-character flag to a Flag.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'D' => Some(Flag::Draft),
            'F' => Some(Flag::Flagged),
            'P' => Some(Flag::Passed),
            'R' => Some(Flag::Replied),
            'S' => Some(Flag::Seen),
            'T' => Some(Flag::Trashed),
            _ => None,
        }
    }
}

/// Parse a mu flag string (e.g., "SFR") into a Vec<Flag>.
pub fn flags_from_string(s: &str) -> Vec<Flag> {
    s.chars().filter_map(Flag::from_char).collect()
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

    /// Convert flags to mu's single-character flag string format.
    /// D=Draft, F=Flagged, N=New, P=Passed, R=Replied, S=Seen, T=Trashed
    pub fn flags_string(&self) -> String {
        let mut s = String::new();
        for flag in &self.flags {
            match flag {
                Flag::Draft => s.push('D'),
                Flag::Flagged => s.push('F'),
                Flag::Passed => s.push('P'),
                Flag::Replied => s.push('R'),
                Flag::Seen => s.push('S'),
                Flag::Trashed => s.push('T'),
                Flag::List | Flag::Unread => {} // not single-char mu flags
            }
        }
        s
    }

    /// Short form for the envelope list (name only, falls back to email).
    pub fn from_display(&self) -> String {
        self.from
            .first()
            .map(|a| a.short_display())
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
