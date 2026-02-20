use anyhow::{Context, Result};
use lettre::message::{Mailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::SmtpConfig;

/// Generate a unique Message-ID for outgoing messages.
fn generate_message_id(from_domain: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let random: u64 = rand_u64();
    format!("<{}.{}@{}>", timestamp, random, from_domain)
}

/// Simple pseudo-random u64 using time + pid for uniqueness.
fn rand_u64() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;
    t.wrapping_mul(6364136223846793005).wrapping_add(pid)
}

/// Parsed compose-file content split at the first blank line.
pub struct ParsedMessage {
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Parse a composed message (as written by the editor) into headers and body.
/// Headers and body are separated by the first blank line.
pub fn parse_composed_message(content: &str) -> Result<ParsedMessage> {
    let mut headers = Vec::new();
    let mut lines = content.lines().peekable();
    let mut current_header: Option<(String, String)> = None;

    // Parse headers (lines before the first blank line).
    // Supports continuation lines (leading whitespace).
    while let Some(line) = lines.peek() {
        if line.is_empty() {
            // Blank line: end of headers
            lines.next();
            break;
        }

        let line = *line;
        lines.next();

        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of previous header
            if let Some((_, ref mut val)) = current_header {
                val.push(' ');
                val.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            // Flush previous header
            if let Some(h) = current_header.take() {
                headers.push(h);
            }
            current_header = Some((name.trim().to_string(), value.trim().to_string()));
        }
    }

    // Flush last header
    if let Some(h) = current_header.take() {
        headers.push(h);
    }

    // Everything after the blank line is body
    let body: String = lines.collect::<Vec<_>>().join("\n");

    Ok(ParsedMessage { headers, body })
}

/// Retrieve SMTP password: run password_command if set, otherwise use plain password.
fn get_password(config: &SmtpConfig) -> Result<String> {
    if let Some(ref cmd) = config.password_command {
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .output()
            .with_context(|| format!("failed to run password command: {}", cmd))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("password command failed: {}", stderr.trim());
        }

        // Take only the first line (standard pass convention: line 1 = password).
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().next().unwrap_or("").trim().to_string())
    } else if let Some(ref pw) = config.password {
        Ok(pw.clone())
    } else {
        anyhow::bail!("no password or password_command configured for SMTP");
    }
}

/// SMTP sender wrapping a lettre async transport.
pub struct SmtpSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl SmtpSender {
    /// Create a new SMTP sender from configuration.
    pub async fn new(config: &SmtpConfig) -> Result<Self> {
        let password = get_password(config)?;
        let creds = Credentials::new(config.username.clone(), password);

        let transport = match config.encryption.as_str() {
            "starttls" => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
                    .with_context(|| {
                        format!("failed to create STARTTLS transport to {}", config.host)
                    })?
                    .port(config.port)
                    .credentials(creds)
                    .build()
            }
            "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                .port(config.port)
                .credentials(creds)
                .build(),
            _ => {
                // "ssl" or any other value: implicit TLS
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
                    .with_context(|| {
                        format!("failed to create TLS transport to {}", config.host)
                    })?
                    .port(config.port)
                    .credentials(creds)
                    .build()
            }
        };

        Ok(Self { transport })
    }

    /// Parse a raw composed message string, build a proper RFC 2822 message,
    /// send it via SMTP, and return the formatted message bytes (for saving
    /// to the Sent folder).
    pub async fn send(&self, raw_message: &str) -> Result<Vec<u8>> {
        let message = build_message(raw_message)?;

        let formatted = message.formatted();

        self.transport
            .send(message)
            .await
            .context("SMTP send failed")?;

        Ok(formatted)
    }
}

/// Build a lettre Message from a raw composed message string, generating a
/// proper Message-ID.
fn build_message(raw_message: &str) -> Result<Message> {
    let parsed = parse_composed_message(raw_message)?;

    let mut builder = MessageBuilder::new();
    let mut from_domain = "localhost".to_string();

    for (name, value) in &parsed.headers {
        match name.to_lowercase().as_str() {
            "from" => {
                let mailbox: Mailbox = value
                    .parse()
                    .with_context(|| format!("invalid From address: {}", value))?;
                // Extract domain for Message-ID generation
                let email_str: &str = mailbox.email.as_ref();
                if let Some(domain) = email_str.split('@').nth(1) {
                    from_domain = domain.to_string();
                }
                builder = builder.from(mailbox);
            }
            "to" => {
                for addr in value.split(',') {
                    let addr = addr.trim();
                    if !addr.is_empty() {
                        let mailbox: Mailbox = addr
                            .parse()
                            .with_context(|| format!("invalid To address: {}", addr))?;
                        builder = builder.to(mailbox);
                    }
                }
            }
            "cc" => {
                for addr in value.split(',') {
                    let addr = addr.trim();
                    if !addr.is_empty() {
                        let mailbox: Mailbox = addr
                            .parse()
                            .with_context(|| format!("invalid Cc address: {}", addr))?;
                        builder = builder.cc(mailbox);
                    }
                }
            }
            "subject" => {
                builder = builder.subject(value.as_str());
            }
            "in-reply-to" => {
                builder = builder.in_reply_to(value.to_string());
            }
            "references" => {
                builder = builder.references(value.to_string());
            }
            "date" => {
                // Let lettre handle date generation; skip user-provided Date
            }
            _ => {
                // Unknown headers are silently ignored for now.
            }
        }
    }

    // Generate a proper Message-ID so replies can reference it
    let msg_id = generate_message_id(&from_domain);
    builder = builder.message_id(Some(msg_id));

    builder
        .body(parsed.body)
        .context("failed to build email message")
}

/// Send a message via SMTP and return the formatted message bytes
/// (for saving to Sent folder).
pub async fn send_message(raw_message: &str, config: &SmtpConfig) -> Result<Vec<u8>> {
    let sender = SmtpSender::new(config).await?;
    sender.send(raw_message).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_composed_message_basic() {
        let input = "From: alice@example.com\n\
                      To: bob@example.com\n\
                      Subject: Hello\n\
                      \n\
                      This is the body.\n\
                      Second line.";

        let parsed = parse_composed_message(input).unwrap();
        assert_eq!(parsed.headers.len(), 3);
        assert_eq!(parsed.headers[0], ("From".to_string(), "alice@example.com".to_string()));
        assert_eq!(parsed.headers[1], ("To".to_string(), "bob@example.com".to_string()));
        assert_eq!(parsed.headers[2], ("Subject".to_string(), "Hello".to_string()));
        assert_eq!(parsed.body, "This is the body.\nSecond line.");
    }

    #[test]
    fn test_parse_composed_message_continuation() {
        let input = "From: alice@example.com\n\
                      References: <a@example.com>\n \
                       <b@example.com>\n\
                      \n\
                      Body here.";

        let parsed = parse_composed_message(input).unwrap();
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(
            parsed.headers[1],
            ("References".to_string(), "<a@example.com> <b@example.com>".to_string())
        );
    }

    #[test]
    fn test_parse_composed_message_empty_body() {
        let input = "From: alice@example.com\n\
                      Subject: Test\n\
                      \n";

        let parsed = parse_composed_message(input).unwrap();
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(parsed.body, "");
    }
}
