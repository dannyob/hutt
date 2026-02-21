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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

impl Default for Envelope {
    fn default() -> Self {
        Self {
            docid: 0,
            message_id: String::new(),
            subject: String::new(),
            from: Vec::new(),
            to: Vec::new(),
            date: Utc::now(),
            flags: Vec::new(),
            maildir: String::new(),
            path: PathBuf::new(),
            thread_meta: ThreadMeta::default(),
        }
    }
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

// ---------------------------------------------------------------------------
// Conversations (grouped threads)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Conversation {
    pub messages: Vec<Envelope>,
}

impl Conversation {
    /// Thread subject from the root message, or the first message's subject.
    pub fn subject(&self) -> &str {
        self.messages
            .first()
            .map(|e| e.subject.as_str())
            .unwrap_or("")
    }

    /// The "representative" message for preview: latest unread, or latest if all read.
    pub fn representative(&self) -> &Envelope {
        self.messages
            .iter()
            .rev()
            .find(|e| e.is_unread())
            .unwrap_or_else(|| self.messages.last().unwrap())
    }

    /// Deduplicated sender display names across the thread.
    pub fn senders(&self) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut names = Vec::new();
        for msg in &self.messages {
            let name = msg.from_display();
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
        names.join(", ")
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn has_unread(&self) -> bool {
        self.messages.iter().any(|e| e.is_unread())
    }

    pub fn has_flagged(&self) -> bool {
        self.messages.iter().any(|e| e.is_flagged())
    }

    /// All docids in this conversation (for triage/multi-select).
    pub fn all_docids(&self) -> Vec<u32> {
        self.messages.iter().map(|e| e.docid).collect()
    }

    /// Date display for the most recent message.
    pub fn date_display(&self) -> String {
        self.messages
            .last()
            .map(|e| e.date_display())
            .unwrap_or_default()
    }
}

/// Group a flat list of envelopes into conversations using thread metadata.
///
/// mu returns envelopes in thread order: a root message (thread_meta.root == true,
/// level == 0) followed by its replies (level > 0). We start a new group each time
/// we see a thread root (root flag set or level drops back to 0).
pub fn group_into_conversations(envelopes: &[Envelope]) -> Vec<Conversation> {
    if envelopes.is_empty() {
        return Vec::new();
    }

    let mut conversations = Vec::new();
    let mut current: Vec<Envelope> = Vec::new();

    for env in envelopes {
        let is_thread_start = env.thread_meta.root || env.thread_meta.level == 0;
        if is_thread_start && !current.is_empty() {
            conversations.push(Conversation {
                messages: std::mem::take(&mut current),
            });
        }
        current.push(env.clone());
    }

    if !current.is_empty() {
        conversations.push(Conversation {
            messages: current,
        });
    }

    conversations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_envelope(docid: u32, subject: &str, level: u32, unread: bool) -> Envelope {
        let mut flags = vec![Flag::Seen];
        if unread {
            flags = vec![];
        }
        Envelope {
            docid,
            subject: subject.to_string(),
            thread_meta: ThreadMeta {
                level,
                root: level == 0,
                thread_subject: level == 0,
            },
            flags,
            from: vec![Address {
                name: Some(format!("User{}", docid)),
                email: format!("user{}@example.com", docid),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn group_empty() {
        let convos = group_into_conversations(&[]);
        assert!(convos.is_empty());
    }

    #[test]
    fn group_single_message() {
        let envelopes = vec![make_envelope(1, "Hello", 0, false)];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos.len(), 1);
        assert_eq!(convos[0].message_count(), 1);
        assert_eq!(convos[0].subject(), "Hello");
    }

    #[test]
    fn group_multi_thread() {
        let envelopes = vec![
            make_envelope(1, "Thread A", 0, false),
            make_envelope(2, "Re: Thread A", 1, true),
            make_envelope(3, "Thread B", 0, false),
            make_envelope(4, "Re: Thread B", 1, false),
            make_envelope(5, "Re: Thread B", 1, true),
        ];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos.len(), 2);

        assert_eq!(convos[0].message_count(), 2);
        assert_eq!(convos[0].subject(), "Thread A");
        assert!(convos[0].has_unread());

        assert_eq!(convos[1].message_count(), 3);
        assert_eq!(convos[1].subject(), "Thread B");
        assert!(convos[1].has_unread());
    }

    #[test]
    fn group_missing_root() {
        // All messages have level > 0 and root=false — everything lumps into one conversation
        let envelopes = vec![
            make_envelope(1, "Orphan A", 1, false),
            make_envelope(2, "Orphan B", 1, true),
        ];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos.len(), 1);
        assert_eq!(convos[0].message_count(), 2);
    }

    #[test]
    fn representative_is_latest_unread() {
        let envelopes = vec![
            make_envelope(1, "Root", 0, false),
            make_envelope(2, "Reply 1", 1, true),
            make_envelope(3, "Reply 2", 1, false),
            make_envelope(4, "Reply 3", 1, true),
        ];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos.len(), 1);
        // Latest unread is docid 4
        assert_eq!(convos[0].representative().docid, 4);
    }

    #[test]
    fn representative_is_latest_when_all_read() {
        let envelopes = vec![
            make_envelope(1, "Root", 0, false),
            make_envelope(2, "Reply", 1, false),
        ];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos[0].representative().docid, 2);
    }

    #[test]
    fn senders_deduplicated() {
        let mut e1 = make_envelope(1, "Root", 0, false);
        e1.from = vec![Address {
            name: Some("Alice".into()),
            email: "alice@example.com".into(),
        }];
        let mut e2 = make_envelope(2, "Reply", 1, false);
        e2.from = vec![Address {
            name: Some("Bob".into()),
            email: "bob@example.com".into(),
        }];
        let mut e3 = make_envelope(3, "Reply 2", 1, false);
        e3.from = vec![Address {
            name: Some("Alice".into()),
            email: "alice@example.com".into(),
        }];
        let convos = group_into_conversations(&[e1, e2, e3]);
        assert_eq!(convos[0].senders(), "Alice, Bob");
    }

    #[test]
    fn all_docids() {
        let envelopes = vec![
            make_envelope(10, "Root", 0, false),
            make_envelope(20, "Reply", 1, false),
        ];
        let convos = group_into_conversations(&envelopes);
        assert_eq!(convos[0].all_docids(), vec![10, 20]);
    }
}
