use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use crate::envelope::{Address, Envelope};

/// What kind of composition are we doing?
#[derive(Debug, Clone)]
pub enum ComposeKind {
    NewMessage,
    Reply,
    ReplyAll,
    Forward,
}

/// What the run loop should do when compose_pending is set.
#[derive(Debug, Clone)]
pub enum ComposePending {
    /// Build context from current selection (normal keybinding path).
    Kind(ComposeKind),
    /// Pre-built context (from IPC compose URL).
    Ready(ComposeContext),
}

/// Everything needed to build the compose buffer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ComposeContext {
    pub kind: ComposeKind,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub subject: String,
    pub quoted_body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub original_path: Option<std::path::PathBuf>,
}

impl ComposeContext {
    /// Build a reply context from an existing envelope + rendered body text.
    pub fn reply(envelope: &Envelope, body_text: &str, reply_all: bool) -> Self {
        let kind = if reply_all {
            ComposeKind::ReplyAll
        } else {
            ComposeKind::Reply
        };

        let subject = if envelope
            .subject
            .to_lowercase()
            .starts_with("re:")
        {
            envelope.subject.clone()
        } else {
            format!("Re: {}", envelope.subject)
        };

        // Quote the body with "> " prefix
        let quoted = body_text
            .lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n");

        // Build references chain: existing References + this Message-Id
        let mut references = Vec::new();
        // We'd populate from the original message headers if available;
        // for now just include the message-id.
        references.push(envelope.message_id.clone());

        Self {
            kind,
            to: envelope.from.clone(),
            cc: if reply_all {
                envelope.to.clone()
            } else {
                Vec::new()
            },
            subject,
            quoted_body: quoted,
            in_reply_to: Some(envelope.message_id.clone()),
            references,
            original_path: Some(envelope.path.clone()),
        }
    }

    /// Build a forward context from an existing envelope + rendered body text.
    pub fn forward(envelope: &Envelope, body_text: &str) -> Self {
        let subject = if envelope
            .subject
            .to_lowercase()
            .starts_with("fwd:")
        {
            envelope.subject.clone()
        } else {
            format!("Fwd: {}", envelope.subject)
        };

        let forwarded_body = format!(
            "---------- Forwarded message ----------\n\
             From: {}\n\
             Date: {}\n\
             Subject: {}\n\n\
             {}",
            format_address_list(&envelope.from),
            envelope.date.format("%a, %b %d, %Y at %H:%M"),
            envelope.subject,
            body_text,
        );

        Self {
            kind: ComposeKind::Forward,
            to: Vec::new(),
            cc: Vec::new(),
            subject,
            quoted_body: forwarded_body,
            in_reply_to: None,
            references: Vec::new(),
            original_path: Some(envelope.path.clone()),
        }
    }

    /// Build a blank new-message context.
    pub fn new_message() -> Self {
        Self {
            kind: ComposeKind::NewMessage,
            to: Vec::new(),
            cc: Vec::new(),
            subject: String::new(),
            quoted_body: String::new(),
            in_reply_to: None,
            references: Vec::new(),
            original_path: None,
        }
    }
}

/// Format a single Address as an RFC 2822 mailbox string.
fn format_address(addr: &Address) -> String {
    match &addr.name {
        Some(name) => format!("{} <{}>", name, addr.email),
        None => addr.email.clone(),
    }
}

/// Format a list of addresses as a comma-separated RFC 2822 string.
fn format_address_list(addrs: &[Address]) -> String {
    addrs
        .iter()
        .map(format_address)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Remove `from_email` from an address list (used for ReplyAll to avoid mailing yourself).
fn remove_self(addrs: &[Address], from_email: &str) -> Vec<Address> {
    addrs
        .iter()
        .filter(|a| !a.email.eq_ignore_ascii_case(from_email))
        .cloned()
        .collect()
}

/// Build the content of the compose temp file: RFC 2822-style headers followed
/// by a blank line and the body.
pub fn build_compose_file(ctx: &ComposeContext, from_email: &str) -> Result<String> {
    let mut out = String::new();

    // From
    out.push_str(&format!("From: {}\n", from_email));

    // To
    match ctx.kind {
        ComposeKind::Reply => {
            out.push_str(&format!("To: {}\n", format_address_list(&ctx.to)));
        }
        ComposeKind::ReplyAll => {
            // To = original From + original To, minus ourselves
            let mut to_addrs = ctx.to.clone();
            to_addrs.extend(remove_self(&ctx.cc, from_email));
            let to_addrs = remove_self(&to_addrs, from_email);
            out.push_str(&format!("To: {}\n", format_address_list(&to_addrs)));
            // Cc = original Cc (if we had it, passed through ctx.cc for ReplyAll
            // is actually the original To; a future iteration may separate these)
        }
        ComposeKind::Forward | ComposeKind::NewMessage => {
            out.push_str(&format!("To: {}\n", format_address_list(&ctx.to)));
        }
    }

    // Cc (for ReplyAll we might have Cc addresses)
    // For now Cc is left empty in the compose buffer for the user to fill in;
    // ReplyAll merges To+Cc into the To line above.

    // Subject
    out.push_str(&format!("Subject: {}\n", ctx.subject));

    // Date
    out.push_str(&format!(
        "Date: {}\n",
        Utc::now().format("%a, %d %b %Y %H:%M:%S %z")
    ));

    // In-Reply-To
    if let Some(ref irt) = ctx.in_reply_to {
        out.push_str(&format!("In-Reply-To: {}\n", irt));
    }

    // References
    if !ctx.references.is_empty() {
        out.push_str(&format!("References: {}\n", ctx.references.join(" ")));
    }

    // Blank line separating headers from body
    out.push('\n');

    // Body
    if !ctx.quoted_body.is_empty() {
        out.push_str(&ctx.quoted_body);
        out.push('\n');
    }

    Ok(out)
}

/// Launch an external editor on the given file path, blocking until the editor
/// exits. Returns `true` if the file was modified (mtime changed).
pub fn launch_editor(file_path: &Path, editor: &str) -> Result<bool> {
    let mtime_before = fs::metadata(file_path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (cmd, args) = parts
        .split_first()
        .context("editor command is empty")?;

    let status = Command::new(cmd)
        .args(args)
        .arg(file_path)
        .status()
        .with_context(|| format!("failed to launch editor: {}", editor))?;

    if !status.success() {
        anyhow::bail!("editor exited with status: {}", status);
    }

    let mtime_after = fs::metadata(file_path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    Ok(mtime_after != mtime_before)
}

/// Launch the editor in a split/new window if running inside kitty or tmux,
/// otherwise fall back to a regular blocking editor launch.
#[allow(dead_code)]
pub fn launch_editor_split(file_path: &Path, editor: &str) -> Result<()> {
    let file_str = file_path
        .to_str()
        .context("file path is not valid UTF-8")?;

    // Try kitty first
    if std::env::var("KITTY_PID").is_ok() {
        let editor_cmd = format!("{} {}", editor, file_str);
        let status = Command::new("kitten")
            .args(["@", "launch", "--type=window", "--", "sh", "-c", &editor_cmd])
            .status()
            .context("failed to launch kitten @ launch")?;

        if status.success() {
            return Ok(());
        }
        // Fall through on failure
    }

    // Try tmux
    if std::env::var("TMUX").is_ok() {
        let editor_cmd = format!("{} {}", editor, file_str);
        let status = Command::new("tmux")
            .args(["split-window", "-h", "-l", "50%", &editor_cmd])
            .status()
            .context("failed to launch tmux split-window")?;

        if status.success() {
            return Ok(());
        }
        // Fall through on failure
    }

    // Fallback: blocking editor
    launch_editor(file_path, editor)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_new_message() {
        let ctx = ComposeContext::new_message();
        let content = build_compose_file(&ctx, "danny@spesh.com").unwrap();
        assert!(content.contains("From: danny@spesh.com"));
        assert!(content.contains("Subject: \n"));
        assert!(content.contains("To: \n"));
    }

    #[test]
    fn test_build_reply() {
        let envelope = Envelope {
            docid: 1,
            message_id: "<abc@example.com>".to_string(),
            subject: "Hello".to_string(),
            from: vec![Address {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }],
            to: vec![Address {
                name: None,
                email: "danny@spesh.com".to_string(),
            }],
            date: Utc::now(),
            flags: vec![],
            maildir: "/Inbox".to_string(),
            path: std::path::PathBuf::from("/tmp/test"),
            thread_meta: crate::envelope::ThreadMeta::default(),
        };

        let ctx = ComposeContext::reply(&envelope, "Hello world\nHow are you?", false);
        let content = build_compose_file(&ctx, "danny@spesh.com").unwrap();

        assert!(content.contains("To: Alice <alice@example.com>"));
        assert!(content.contains("Subject: Re: Hello"));
        assert!(content.contains("In-Reply-To: <abc@example.com>"));
        assert!(content.contains("> Hello world"));
        assert!(content.contains("> How are you?"));
    }

    #[test]
    fn test_build_forward() {
        let envelope = Envelope {
            docid: 1,
            message_id: "<abc@example.com>".to_string(),
            subject: "Hello".to_string(),
            from: vec![Address {
                name: Some("Alice".to_string()),
                email: "alice@example.com".to_string(),
            }],
            to: vec![Address {
                name: None,
                email: "danny@spesh.com".to_string(),
            }],
            date: Utc::now(),
            flags: vec![],
            maildir: "/Inbox".to_string(),
            path: std::path::PathBuf::from("/tmp/test"),
            thread_meta: crate::envelope::ThreadMeta::default(),
        };

        let ctx = ComposeContext::forward(&envelope, "Original body text");
        let content = build_compose_file(&ctx, "danny@spesh.com").unwrap();

        assert!(content.contains("Subject: Fwd: Hello"));
        assert!(content.contains("---------- Forwarded message ----------"));
        assert!(content.contains("Original body text"));
    }

    #[test]
    fn test_format_address() {
        let addr = Address {
            name: Some("Bob".to_string()),
            email: "bob@example.com".to_string(),
        };
        assert_eq!(format_address(&addr), "Bob <bob@example.com>");

        let bare = Address {
            name: None,
            email: "bare@example.com".to_string(),
        };
        assert_eq!(format_address(&bare), "bare@example.com");
    }
}
