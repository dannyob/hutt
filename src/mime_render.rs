use anyhow::{Context, Result};
use html2text::render::RichAnnotation;
use mail_parser::MimeHeaders;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Rich rendering types
// ---------------------------------------------------------------------------

/// A single styled span within a rendered line.
#[derive(Debug, Clone)]
pub struct RichSpan {
    pub text: String,
    pub kind: SpanKind,
}

/// What kind of content a span represents.
#[derive(Debug, Clone)]
pub enum SpanKind {
    Normal,
    Quote,
    Link(String),
    Emphasis,
    Strong,
    Code,
}

/// A clickable link region for mouse hit-testing.
#[derive(Debug, Clone)]
pub struct LinkRegion {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub url: String,
}

/// A fully rendered message with styled spans and link metadata.
#[derive(Debug, Clone)]
pub struct RenderedMessage {
    pub lines: Vec<Vec<RichSpan>>,
    pub links: Vec<LinkRegion>,
}

impl RenderedMessage {
    /// Convert back to plain text (for reply quoting, compose, etc.)
    pub fn to_plain_text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            for span in line {
                out.push_str(&span.text);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Cache of rendered message bodies, keyed by (message_id, width).
pub struct RenderCache {
    cache: HashMap<(String, u16), RenderedMessage>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn get(&self, message_id: &str, width: u16) -> Option<&RenderedMessage> {
        self.cache.get(&(message_id.to_string(), width))
    }

    pub fn insert(&mut self, message_id: String, width: u16, msg: RenderedMessage) {
        self.cache.insert((message_id, width), msg);
    }
}

// ---------------------------------------------------------------------------
// Plain text rendering
// ---------------------------------------------------------------------------

/// Render plain text into a RenderedMessage, detecting URLs and quote lines.
/// Lines are pre-wrapped to `width`.
pub fn render_plain_text(text: &str, width: u16) -> RenderedMessage {
    let mut lines = Vec::new();
    let mut links = Vec::new();
    let width = width as usize;

    for raw_line in text.lines() {
        let is_quote = raw_line.starts_with('>');
        let spans = if is_quote {
            vec![RichSpan {
                text: raw_line.to_string(),
                kind: SpanKind::Quote,
            }]
        } else {
            detect_urls(raw_line)
        };

        let wrapped = wrap_rich_line(&spans, width);
        for wrapped_line in wrapped {
            let line_idx = lines.len();
            let mut col = 0usize;
            for span in &wrapped_line {
                let span_width = span.text.chars().count();
                if let SpanKind::Link(ref url) = span.kind {
                    links.push(LinkRegion {
                        line: line_idx,
                        col_start: col,
                        col_end: col + span_width,
                        url: url.clone(),
                    });
                }
                col += span_width;
            }
            lines.push(wrapped_line);
        }
    }

    RenderedMessage { lines, links }
}

/// Scan a line for URLs and split into Normal / Link spans.
fn detect_urls(line: &str) -> Vec<RichSpan> {
    let mut spans = Vec::new();
    let mut remaining = line;
    let url_starts = ["https://", "http://", "mid:", "mailto:"];

    while !remaining.is_empty() {
        let earliest = url_starts
            .iter()
            .filter_map(|prefix| remaining.find(prefix).map(|pos| (pos, *prefix)))
            .min_by_key(|(pos, _)| *pos);

        match earliest {
            Some((pos, _prefix)) => {
                if pos > 0 {
                    spans.push(RichSpan {
                        text: remaining[..pos].to_string(),
                        kind: SpanKind::Normal,
                    });
                }
                let url_start = &remaining[pos..];
                let url_end = url_start
                    .find(|c: char| {
                        c.is_whitespace() || c == '>' || c == ')' || c == ']'
                    })
                    .unwrap_or(url_start.len());
                let url = url_start[..url_end]
                    .trim_end_matches(|c: char| {
                        c == '.' || c == ',' || c == ';' || c == '!' || c == '?'
                    });
                let url_len = url.len();
                spans.push(RichSpan {
                    text: url.to_string(),
                    kind: SpanKind::Link(url.to_string()),
                });
                remaining = &remaining[pos + url_len..];
            }
            None => {
                spans.push(RichSpan {
                    text: remaining.to_string(),
                    kind: SpanKind::Normal,
                });
                break;
            }
        }
    }
    spans
}

/// Word-wrap a sequence of RichSpans to fit within `max_width` characters.
fn wrap_rich_line(spans: &[RichSpan], max_width: usize) -> Vec<Vec<RichSpan>> {
    if max_width == 0 {
        return vec![vec![RichSpan {
            text: String::new(),
            kind: SpanKind::Normal,
        }]];
    }

    let total: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if total <= max_width {
        return vec![spans.to_vec()];
    }

    // Flatten into chars with kind tags, then split at width boundaries
    let mut tagged_chars: Vec<(char, SpanKind)> = Vec::new();
    for span in spans {
        for ch in span.text.chars() {
            tagged_chars.push((ch, span.kind.clone()));
        }
    }

    let mut result: Vec<Vec<RichSpan>> = Vec::new();
    let mut pos = 0;

    while pos < tagged_chars.len() {
        let remaining = tagged_chars.len() - pos;
        let chunk_len = if remaining <= max_width {
            remaining
        } else {
            tagged_chars[pos..pos + max_width]
                .iter()
                .rposition(|(c, _)| *c == ' ' || *c == '-' || *c == '/')
                .map(|i| i + 1)
                .unwrap_or(max_width)
        };

        let mut line_spans: Vec<RichSpan> = Vec::new();
        for (ch, kind) in &tagged_chars[pos..pos + chunk_len] {
            if let Some(last) = line_spans.last_mut() {
                if same_kind(&last.kind, kind) {
                    last.text.push(*ch);
                    continue;
                }
            }
            line_spans.push(RichSpan {
                text: ch.to_string(),
                kind: kind.clone(),
            });
        }
        result.push(line_spans);
        pos += chunk_len;
    }

    if result.is_empty() {
        result.push(vec![RichSpan {
            text: String::new(),
            kind: SpanKind::Normal,
        }]);
    }
    result
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

/// Render HTML bytes into a RenderedMessage using html2text's rich annotations.
pub fn render_html(html: &[u8], width: u16) -> RenderedMessage {
    let width_usize = width as usize;
    let tagged_lines = match html2text::from_read_rich(html, width_usize) {
        Ok(lines) => lines,
        Err(_) => {
            return RenderedMessage {
                lines: vec![vec![RichSpan {
                    text: "[HTML rendering error]".to_string(),
                    kind: SpanKind::Normal,
                }]],
                links: Vec::new(),
            };
        }
    };

    let mut lines: Vec<Vec<RichSpan>> = Vec::new();
    let mut links: Vec<LinkRegion> = Vec::new();

    for tagged_line in &tagged_lines {
        let line_idx = lines.len();
        let mut col = 0usize;
        let mut spans: Vec<RichSpan> = Vec::new();

        for ts in tagged_line.tagged_strings() {
            let kind = annotations_to_kind(&ts.tag);
            let span_width = ts.s.chars().count();

            if let SpanKind::Link(ref url) = kind {
                links.push(LinkRegion {
                    line: line_idx,
                    col_start: col,
                    col_end: col + span_width,
                    url: url.clone(),
                });
            }
            col += span_width;

            if let Some(last) = spans.last_mut() {
                if same_kind(&last.kind, &kind) {
                    last.text.push_str(&ts.s);
                    continue;
                }
            }
            spans.push(RichSpan {
                text: ts.s.to_string(),
                kind,
            });
        }
        lines.push(spans);
    }

    RenderedMessage { lines, links }
}

/// Map html2text rich annotations to SpanKind.
fn annotations_to_kind(annotations: &[RichAnnotation]) -> SpanKind {
    for ann in annotations.iter().rev() {
        match ann {
            RichAnnotation::Link(url) => return SpanKind::Link(url.clone()),
            RichAnnotation::Emphasis => return SpanKind::Emphasis,
            RichAnnotation::Strong => return SpanKind::Strong,
            RichAnnotation::Code | RichAnnotation::Preformat(_) => return SpanKind::Code,
            RichAnnotation::Image(src) => return SpanKind::Link(src.clone()),
            _ => {}
        }
    }
    SpanKind::Normal
}

/// Check if two SpanKinds are the "same" for merging purposes.
fn same_kind(a: &SpanKind, b: &SpanKind) -> bool {
    match (a, b) {
        (SpanKind::Normal, SpanKind::Normal) => true,
        (SpanKind::Quote, SpanKind::Quote) => true,
        (SpanKind::Emphasis, SpanKind::Emphasis) => true,
        (SpanKind::Strong, SpanKind::Strong) => true,
        (SpanKind::Code, SpanKind::Code) => true,
        (SpanKind::Link(a), SpanKind::Link(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Attachment discovery and extraction
// ---------------------------------------------------------------------------

/// An extracted attachment ready to save or open.
pub struct ExtractedAttachment {
    pub filename: String,
    #[allow(dead_code)]
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Info about a discovered attachment (for rendering the attachment list).
struct AttachmentInfo {
    filename: String,
    mime_type: String,
    size: usize,
    content_id: String,
}

/// Extract an attachment from a message file by content-id.
/// Content-id can be a MIME Content-ID header value or "part.N" (part index).
pub fn extract_attachment(message_path: &Path, content_id: &str) -> Result<ExtractedAttachment> {
    let raw = std::fs::read(message_path)
        .with_context(|| format!("reading message: {}", message_path.display()))?;
    extract_attachment_from_bytes(&raw, content_id)
}

/// Extract an attachment from raw message bytes by content-id.
pub fn extract_attachment_from_bytes(raw: &[u8], content_id: &str) -> Result<ExtractedAttachment> {
    let message = mail_parser::MessageParser::default()
        .parse(raw)
        .context("failed to parse MIME message")?;

    // Try matching by Content-ID header first
    for (idx, part) in message.parts.iter().enumerate() {
        if let Some(cid) = part.content_id() {
            let cid_clean = cid.trim_matches(|c| c == '<' || c == '>');
            if cid_clean == content_id {
                return extract_part(part, idx);
            }
        }
    }

    // Try matching by positional index: "part.N"
    if let Some(n_str) = content_id.strip_prefix("part.") {
        if let Ok(n) = n_str.parse::<usize>() {
            if let Some(part) = message.parts.get(n) {
                return extract_part(part, n);
            }
        }
    }

    anyhow::bail!("attachment not found: {}", content_id)
}

fn extract_part(part: &mail_parser::MessagePart, idx: usize) -> Result<ExtractedAttachment> {
    let filename = part
        .attachment_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("attachment-{}", idx));

    let mime_type = part
        .content_type()
        .map(|ct| {
            if let Some(subtype) = ct.subtype() {
                format!("{}/{}", ct.ctype(), subtype)
            } else {
                ct.ctype().to_string()
            }
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let data = match &part.body {
        mail_parser::PartType::Binary(cow) | mail_parser::PartType::InlineBinary(cow) => {
            cow.to_vec()
        }
        mail_parser::PartType::Text(cow) => cow.as_bytes().to_vec(),
        mail_parser::PartType::Html(cow) => cow.as_bytes().to_vec(),
        mail_parser::PartType::Message(msg) => {
            format!(
                "Nested message: {}",
                msg.subject().unwrap_or("(no subject)")
            )
            .into_bytes()
        }
        mail_parser::PartType::Multipart(_) => {
            anyhow::bail!("cannot extract multipart container as attachment");
        }
    };

    Ok(ExtractedAttachment {
        filename,
        mime_type,
        data,
    })
}

fn discover_attachments(message: &mail_parser::Message) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();

    for (idx, part) in message.parts.iter().enumerate() {
        match &part.body {
            mail_parser::PartType::Multipart(_) => continue,
            mail_parser::PartType::Text(_) if part.is_content_type("text", "plain") => {
                if idx <= 1 {
                    continue;
                }
                if part.attachment_name().is_none() {
                    continue;
                }
            }
            mail_parser::PartType::Html(_) => {
                if idx <= 1 {
                    continue;
                }
                if part.attachment_name().is_none() {
                    continue;
                }
            }
            _ => {}
        }

        let filename = part
            .attachment_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("attachment-{}", idx));

        let mime_type = part
            .content_type()
            .map(|ct| {
                if let Some(subtype) = ct.subtype() {
                    format!("{}/{}", ct.ctype(), subtype)
                } else {
                    ct.ctype().to_string()
                }
            })
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let size = match &part.body {
            mail_parser::PartType::Binary(cow) | mail_parser::PartType::InlineBinary(cow) => {
                cow.len()
            }
            mail_parser::PartType::Text(cow) => cow.len(),
            mail_parser::PartType::Html(cow) => cow.len(),
            _ => 0,
        };

        let content_id = part
            .content_id()
            .map(|cid| cid.trim_matches(|c| c == '<' || c == '>').to_string())
            .unwrap_or_else(|| format!("part.{}", idx));

        attachments.push(AttachmentInfo {
            filename,
            mime_type,
            size,
            content_id,
        });
    }

    attachments
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn append_attachment_list(
    rendered: &mut RenderedMessage,
    attachments: &[AttachmentInfo],
    message_id: &str,
    width: u16,
) {
    rendered.lines.push(Vec::new());
    let sep_width = (width as usize).min(50);
    let separator = format!("── Attachments {}", "─".repeat(sep_width.saturating_sub(15)));
    rendered.lines.push(vec![RichSpan {
        text: separator,
        kind: SpanKind::Normal,
    }]);

    for att in attachments {
        let line_idx = rendered.lines.len();
        let url = format!("mid:{}/{}", message_id, att.content_id);
        let label = format!(
            "📎 {} ({}, {})",
            att.filename,
            att.mime_type,
            format_size(att.size)
        );
        let col_end = label.chars().count();

        rendered.links.push(LinkRegion {
            line: line_idx,
            col_start: 0,
            col_end,
            url: url.clone(),
        });

        rendered.lines.push(vec![RichSpan {
            text: label,
            kind: SpanKind::Link(url),
        }]);
    }
}

// ---------------------------------------------------------------------------
// Top-level render entry points
// ---------------------------------------------------------------------------

/// Render from raw bytes (testable without filesystem).
pub fn render_message_from_bytes(
    raw: &[u8],
    message_id: &str,
    width: u16,
) -> Result<RenderedMessage> {
    let message = mail_parser::MessageParser::default()
        .parse(raw)
        .context("failed to parse MIME message")?;

    let mut rendered = if let Some(text) = message.body_text(0) {
        render_plain_text(&text, width)
    } else if let Some(html) = message.body_html(0) {
        render_html(html.as_bytes(), width)
    } else {
        let mut found = None;
        for part in message.parts.iter() {
            if let mail_parser::PartType::Text(text) = &part.body {
                if part.is_content_type("text", "plain") {
                    found = Some(render_plain_text(text, width));
                    break;
                }
            }
        }
        if found.is_none() {
            for part in message.parts.iter() {
                if let mail_parser::PartType::Text(text) = &part.body {
                    if part.is_content_type("text", "html") {
                        found = Some(render_html(text.as_bytes(), width));
                        break;
                    }
                }
            }
        }
        found.unwrap_or_else(|| RenderedMessage {
            lines: vec![vec![RichSpan {
                text: "[No text content]".to_string(),
                kind: SpanKind::Normal,
            }]],
            links: Vec::new(),
        })
    };

    let attachments = discover_attachments(&message);
    if !attachments.is_empty() {
        append_attachment_list(&mut rendered, &attachments, message_id, width);
    }

    Ok(rendered)
}

/// Render a message file to a RenderedMessage for the preview/thread panes.
pub fn render_message(path: &Path, message_id: &str, width: u16) -> Result<RenderedMessage> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading message file: {}", path.display()))?;
    render_message_from_bytes(&raw, message_id, width)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Plain text ──────────────────────────────────────────────

    #[test]
    fn plain_text_basic() {
        let rm = render_plain_text("Hello world\nSecond line", 80);
        assert_eq!(rm.lines.len(), 2);
        assert_eq!(rm.lines[0].len(), 1);
        assert_eq!(rm.lines[0][0].text, "Hello world");
        assert!(matches!(rm.lines[0][0].kind, SpanKind::Normal));
        assert!(rm.links.is_empty());
    }

    #[test]
    fn plain_text_quote_detection() {
        let rm = render_plain_text("> quoted text\nnormal", 80);
        assert!(matches!(rm.lines[0][0].kind, SpanKind::Quote));
        assert!(matches!(rm.lines[1][0].kind, SpanKind::Normal));
    }

    #[test]
    fn plain_text_url_detection() {
        let rm = render_plain_text("See https://example.com for details", 80);
        assert_eq!(rm.lines[0].len(), 3);
        assert!(
            matches!(&rm.lines[0][1].kind, SpanKind::Link(url) if url == "https://example.com")
        );
        assert_eq!(rm.links.len(), 1);
        assert_eq!(rm.links[0].url, "https://example.com");
        assert_eq!(rm.links[0].col_start, 4);
        assert_eq!(rm.links[0].col_end, 23);
    }

    #[test]
    fn plain_text_multiple_urls() {
        let rm = render_plain_text("A http://a.com B https://b.com C", 80);
        assert_eq!(rm.links.len(), 2);
        assert_eq!(rm.links[0].url, "http://a.com");
        assert_eq!(rm.links[1].url, "https://b.com");
    }

    #[test]
    fn plain_text_wrapping() {
        let rm = render_plain_text("hello world foo", 10);
        assert!(rm.lines.len() >= 2);
    }

    #[test]
    fn plain_text_to_plain_text_roundtrip() {
        let rm = render_plain_text("line one\nline two\nline three", 80);
        assert_eq!(rm.to_plain_text(), "line one\nline two\nline three");
    }

    // ── HTML ────────────────────────────────────────────────────

    #[test]
    fn html_basic() {
        let html = b"<p>Hello <b>world</b></p>";
        let rm = render_html(html, 80);
        assert!(!rm.lines.is_empty());
        let text = rm.to_plain_text();
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn html_link_detection() {
        let html = b"<p>Click <a href=\"https://example.com\">here</a> now</p>";
        let rm = render_html(html, 80);
        assert!(!rm.links.is_empty());
        assert_eq!(rm.links[0].url, "https://example.com");
        let link_span = rm.lines[rm.links[0].line]
            .iter()
            .find(|s| matches!(&s.kind, SpanKind::Link(_)));
        assert!(link_span.is_some());
        assert!(link_span.unwrap().text.contains("here"));
    }

    #[test]
    fn html_emphasis() {
        let html = b"<p><em>italic</em> and <strong>bold</strong></p>";
        let rm = render_html(html, 80);
        let has_emphasis = rm
            .lines
            .iter()
            .any(|line| line.iter().any(|s| matches!(s.kind, SpanKind::Emphasis)));
        let has_strong = rm
            .lines
            .iter()
            .any(|line| line.iter().any(|s| matches!(s.kind, SpanKind::Strong)));
        assert!(has_emphasis);
        assert!(has_strong);
    }

    // ── Attachments ─────────────────────────────────────────────

    #[test]
    fn extract_attachment_by_index() {
        let msg = concat!(
            "From: test@example.com\r\n",
            "To: user@example.com\r\n",
            "Subject: test\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"bound\"\r\n",
            "\r\n",
            "--bound\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "Hello\r\n",
            "--bound\r\n",
            "Content-Type: application/pdf\r\n",
            "Content-Disposition: attachment; filename=\"report.pdf\"\r\n",
            "\r\n",
            "fake pdf content\r\n",
            "--bound--\r\n",
        );
        let result = extract_attachment_from_bytes(msg.as_bytes(), "part.2");
        assert!(result.is_ok());
        let att = result.unwrap();
        assert_eq!(att.filename, "report.pdf");
        assert!(att.mime_type.contains("pdf"));
    }

    #[test]
    fn render_message_with_attachments() {
        let msg = concat!(
            "From: test@example.com\r\n",
            "To: user@example.com\r\n",
            "Subject: test\r\n",
            "Message-ID: <test@example.com>\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"bound\"\r\n",
            "\r\n",
            "--bound\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "Hello\r\n",
            "--bound\r\n",
            "Content-Type: application/pdf\r\n",
            "Content-Disposition: attachment; filename=\"report.pdf\"\r\n",
            "\r\n",
            "fake pdf content\r\n",
            "--bound--\r\n",
        );
        let rm = render_message_from_bytes(msg.as_bytes(), "test@example.com", 80).unwrap();
        let text = rm.to_plain_text();
        assert!(text.contains("Attachments"));
        assert!(text.contains("report.pdf"));
        let att_links: Vec<_> = rm.links.iter().filter(|l| l.url.starts_with("mid:")).collect();
        assert!(!att_links.is_empty());
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
    }
}
