use anyhow::{Context, Result};
use mail_parser::MimeHeaders;
use std::collections::HashMap;
use std::path::Path;

/// Cache of rendered message bodies, keyed by (message_id, width).
pub struct RenderCache {
    cache: HashMap<(String, u16), String>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn get(&self, message_id: &str, width: u16) -> Option<&str> {
        self.cache
            .get(&(message_id.to_string(), width))
            .map(|s| s.as_str())
    }

    pub fn insert(&mut self, message_id: String, width: u16, text: String) {
        self.cache.insert((message_id, width), text);
    }
}

fn html_to_text(html: &[u8], width: usize) -> String {
    html2text::from_read(html, width).unwrap_or_else(|_| "[HTML rendering error]".to_string())
}

/// Render a message file to plain text for the preview pane.
pub fn render_message(path: &Path, width: u16) -> Result<String> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading message file: {}", path.display()))?;

    let message = mail_parser::MessageParser::default()
        .parse(&raw)
        .context("failed to parse MIME message")?;

    // Prefer text/plain, fall back to text/html
    if let Some(text) = message.body_text(0) {
        return Ok(text.to_string());
    }

    if let Some(html) = message.body_html(0) {
        return Ok(html_to_text(html.as_bytes(), width as usize));
    }

    // Check for multipart with nested text parts
    for part in message.parts.iter() {
        if let mail_parser::PartType::Text(text) = &part.body {
            if part.is_content_type("text", "plain") {
                return Ok(text.to_string());
            }
        }
    }

    for part in message.parts.iter() {
        if let mail_parser::PartType::Text(text) = &part.body {
            if part.is_content_type("text", "html") {
                return Ok(html_to_text(text.as_bytes(), width as usize));
            }
        }
    }

    Ok("[No text content]".to_string())
}
