# Clickable Links & Attachments Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the plain-string message renderer with a rich `RenderedMessage` abstraction that tracks clickable link regions, then add attachment display and click-to-open.

**Architecture:** `mime_render.rs` produces `RenderedMessage` (pre-wrapped styled spans + link regions). Preview and thread widgets consume `RenderedMessage` and map `SpanKind` to ratatui styles. Mouse clicks in content areas hit-test against link regions and dispatch by URL scheme. Attachments appear as a footer section with `mid:message-id/content-id` links.

**Tech Stack:** `html2text::from_read_rich` (rich annotations), `mail-parser` (MIME parsing, attachment extraction), ratatui (rendering), existing `links.rs` (URL dispatch)

---

### Task 1: RenderedMessage data types

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Add the new types below the existing imports in `src/mime_render.rs`**

Add after the existing `use` block:

```rust
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
    Link(String),  // carries target URL
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
```

**Step 2: Run `cargo check`**

```bash
cargo check
```

Expected: compiles (new types are defined but not yet used — that's fine, dead_code warnings expected).

**Step 3: Commit**

```bash
git add src/mime_render.rs
git commit -m "Add RenderedMessage, RichSpan, SpanKind, LinkRegion types"
```

---

### Task 2: Plain text rendering with URL detection

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Write test for plain text rendering**

Add to `src/mime_render.rs` at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(rm.lines[0].len(), 3); // "See " + link + " for details"
        assert!(matches!(&rm.lines[0][1].kind, SpanKind::Link(url) if url == "https://example.com"));
        assert_eq!(rm.links.len(), 1);
        assert_eq!(rm.links[0].url, "https://example.com");
        assert_eq!(rm.links[0].col_start, 4);
        assert_eq!(rm.links[0].col_end, 24);
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
        // 10-char width forces wrap
        let rm = render_plain_text("hello world foo", 10);
        assert!(rm.lines.len() >= 2); // wrapped
    }
}
```

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib mime_render::tests -- 2>&1 | head -20
```

Expected: FAIL — `render_plain_text` does not exist.

**Step 3: Implement `render_plain_text` and word-wrapping**

Add to `src/mime_render.rs`:

```rust
/// Render plain text into a RenderedMessage, detecting URLs and quote lines.
/// Lines are pre-wrapped to `width`.
pub fn render_plain_text(text: &str, width: u16) -> RenderedMessage {
    let mut lines = Vec::new();
    let mut links = Vec::new();
    let width = width as usize;

    for raw_line in text.lines() {
        let is_quote = raw_line.starts_with('>');
        let spans_with_links = if is_quote {
            vec![RichSpan { text: raw_line.to_string(), kind: SpanKind::Quote }]
        } else {
            detect_urls(raw_line)
        };

        // Word-wrap the spans, tracking column positions for link regions
        let wrapped = wrap_rich_line(&spans_with_links, width);
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
        // Find the earliest URL start
        let earliest = url_starts.iter()
            .filter_map(|prefix| remaining.find(prefix).map(|pos| (pos, *prefix)))
            .min_by_key(|(pos, _)| *pos);

        match earliest {
            Some((pos, _prefix)) => {
                // Text before the URL
                if pos > 0 {
                    spans.push(RichSpan {
                        text: remaining[..pos].to_string(),
                        kind: SpanKind::Normal,
                    });
                }
                // Find URL end (whitespace, angle bracket, or end of string)
                let url_start = &remaining[pos..];
                let url_end = url_start.find(|c: char| c.is_whitespace() || c == '>' || c == ')' || c == ']')
                    .unwrap_or(url_start.len());
                // Strip trailing punctuation that's likely not part of the URL
                let url = url_start[..url_end].trim_end_matches(|c: char| c == '.' || c == ',' || c == ';' || c == '!' || c == '?');
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
/// Returns a Vec of lines, each a Vec of RichSpans.
fn wrap_rich_line(spans: &[RichSpan], max_width: usize) -> Vec<Vec<RichSpan>> {
    if max_width == 0 {
        return vec![vec![RichSpan { text: String::new(), kind: SpanKind::Normal }]];
    }

    // Concatenate to measure total width
    let total: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if total <= max_width {
        return vec![spans.to_vec()];
    }

    // Simple strategy: flatten into chars with kind tags, then split at width boundaries
    // preferring word breaks
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
            // Try to find a word boundary
            let search_end = pos + max_width;
            let break_at = tagged_chars[pos..search_end].iter()
                .rposition(|(c, _)| *c == ' ' || *c == '-' || *c == '/')
                .map(|i| i + 1)
                .unwrap_or(max_width);
            break_at
        };

        // Build spans for this chunk, merging consecutive chars with same kind
        let mut line_spans: Vec<RichSpan> = Vec::new();
        for i in pos..pos + chunk_len {
            let (ch, ref kind) = tagged_chars[i];
            if let Some(last) = line_spans.last_mut() {
                if std::mem::discriminant(&last.kind) == std::mem::discriminant(kind) {
                    // Check if URLs match for Link variant
                    let same = match (&last.kind, kind) {
                        (SpanKind::Link(a), SpanKind::Link(b)) => a == b,
                        _ => true,
                    };
                    if same {
                        last.text.push(ch);
                        continue;
                    }
                }
            }
            line_spans.push(RichSpan { text: ch.to_string(), kind: kind.clone() });
        }
        result.push(line_spans);
        pos += chunk_len;
    }

    if result.is_empty() {
        result.push(vec![RichSpan { text: String::new(), kind: SpanKind::Normal }]);
    }
    result
}
```

**Step 4: Run tests to verify they pass**

```bash
cargo test --lib mime_render::tests
```

Expected: all PASS.

**Step 5: Commit**

```bash
git add src/mime_render.rs
git commit -m "Implement render_plain_text with URL detection and word wrapping"
```

---

### Task 3: HTML rendering with rich annotations

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Write tests for HTML rendering**

Add to the test module:

```rust
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
        // The link text should be "here"
        let link_span = rm.lines[rm.links[0].line].iter()
            .find(|s| matches!(&s.kind, SpanKind::Link(_)));
        assert!(link_span.is_some());
        assert!(link_span.unwrap().text.contains("here"));
    }

    #[test]
    fn html_emphasis() {
        let html = b"<p><em>italic</em> and <strong>bold</strong></p>";
        let rm = render_html(html, 80);
        let has_emphasis = rm.lines.iter().any(|line|
            line.iter().any(|s| matches!(s.kind, SpanKind::Emphasis)));
        let has_strong = rm.lines.iter().any(|line|
            line.iter().any(|s| matches!(s.kind, SpanKind::Strong)));
        assert!(has_emphasis);
        assert!(has_strong);
    }
```

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib mime_render::tests::html 2>&1 | head -10
```

Expected: FAIL — `render_html` does not exist.

**Step 3: Implement `render_html`**

Add to `src/mime_render.rs`:

```rust
use html2text::render::text_renderer::RichAnnotation;

/// Render HTML bytes into a RenderedMessage using html2text's rich annotation mode.
pub fn render_html(html: &[u8], width: u16) -> RenderedMessage {
    let width = width as usize;
    let tagged_lines = match html2text::from_read_rich(html, width) {
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

            // Try to merge with previous span of same kind
            if let Some(last) = spans.last_mut() {
                if same_kind(&last.kind, &kind) {
                    last.text.push_str(&ts.s);
                    continue;
                }
            }
            spans.push(RichSpan { text: ts.s.to_string(), kind });
        }
        lines.push(spans);
    }

    RenderedMessage { lines, links }
}

/// Map html2text rich annotations to our SpanKind.
/// Takes the "innermost" meaningful annotation.
fn annotations_to_kind(annotations: &[RichAnnotation]) -> SpanKind {
    // Walk from innermost (last) to outermost
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
```

**Step 4: Run tests**

```bash
cargo test --lib mime_render::tests
```

Expected: all PASS.

**Step 5: Commit**

```bash
git add src/mime_render.rs
git commit -m "Implement render_html with rich annotations from html2text"
```

---

### Task 4: Update render_message and RenderCache

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Update `render_message` to return `RenderedMessage`**

Replace the existing `render_message` function:

```rust
/// Render a message file to a RenderedMessage for the preview/thread panes.
pub fn render_message(path: &Path, width: u16) -> Result<RenderedMessage> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading message file: {}", path.display()))?;

    let message = mail_parser::MessageParser::default()
        .parse(&raw)
        .context("failed to parse MIME message")?;

    // Prefer text/plain, fall back to text/html
    if let Some(text) = message.body_text(0) {
        return Ok(render_plain_text(&text, width));
    }

    if let Some(html) = message.body_html(0) {
        return Ok(render_html(html.as_bytes(), width));
    }

    // Check for multipart with nested text parts
    for part in message.parts.iter() {
        if let mail_parser::PartType::Text(text) = &part.body {
            if part.is_content_type("text", "plain") {
                return Ok(render_plain_text(text, width));
            }
        }
    }

    for part in message.parts.iter() {
        if let mail_parser::PartType::Text(text) = &part.body {
            if part.is_content_type("text", "html") {
                return Ok(render_html(text.as_bytes(), width));
            }
        }
    }

    Ok(RenderedMessage {
        lines: vec![vec![RichSpan {
            text: "[No text content]".to_string(),
            kind: SpanKind::Normal,
        }]],
        links: Vec::new(),
    })
}
```

Remove the old `html_to_text` helper (no longer needed).

**Step 2: Update `RenderCache` to store `RenderedMessage`**

```rust
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
```

**Step 3: Run `cargo check` to see all the call-site breakages**

```bash
cargo check 2>&1 | grep "error" | head -20
```

Expected: type mismatch errors in `tui/mod.rs`, `tui/preview.rs`, `tui/thread_view.rs` where code expects `String` / `&str` but now gets `RenderedMessage` / `&RenderedMessage`.

**Step 4: Commit (partial — breaks downstream, that's fine, we fix in next tasks)**

```bash
git add src/mime_render.rs
git commit -m "Update render_message to return RenderedMessage, update RenderCache"
```

---

### Task 5: Update PreviewPane widget

**Files:**
- Modify: `src/tui/preview.rs`
- Modify: `src/tui/mod.rs` (preview_cache usage)

**Step 1: Update PreviewPane to accept `&RenderedMessage`**

Rewrite `src/tui/preview.rs`:

```rust
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::envelope::Envelope;
use crate::mime_render::{RenderedMessage, SpanKind};

pub struct PreviewPane<'a> {
    pub envelope: Option<&'a Envelope>,
    pub body: Option<&'a RenderedMessage>,
    pub scroll: u16,
}

impl<'a> Widget for PreviewPane<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let envelope = match self.envelope {
            Some(e) => e,
            None => {
                let style = Style::default().fg(Color::DarkGray);
                buf.set_string(
                    area.x + 2,
                    area.y + area.height / 2,
                    "No message selected",
                    style,
                );
                return;
            }
        };

        // Build header lines
        let header_style = Style::default().fg(Color::DarkGray);
        let value_style = Style::default().fg(Color::White);
        let subject_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Subject: ", header_style),
                Span::styled(&envelope.subject, subject_style),
            ]),
            Line::from(vec![
                Span::styled("From:    ", header_style),
                Span::styled(
                    envelope.from.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "),
                    value_style,
                ),
            ]),
            Line::from(vec![
                Span::styled("To:      ", header_style),
                Span::styled(
                    envelope.to.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "),
                    value_style,
                ),
            ]),
            Line::from(vec![
                Span::styled("Date:    ", header_style),
                Span::styled(
                    envelope.date.format("%Y-%m-%d %H:%M %Z").to_string(),
                    value_style,
                ),
            ]),
            Line::from(""), // separator
        ];

        // Add body lines from RenderedMessage
        if let Some(body) = self.body {
            for rich_line in &body.lines {
                let spans: Vec<Span> = rich_line.iter().map(|s| {
                    Span::styled(s.text.clone(), span_style(&s.kind))
                }).collect();
                lines.push(Line::from(spans));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "Loading…",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .scroll((self.scroll, 0));

        paragraph.render(area, buf);
    }
}

/// Map SpanKind to ratatui Style.
pub fn span_style(kind: &SpanKind) -> Style {
    match kind {
        SpanKind::Normal => Style::default().fg(Color::White),
        SpanKind::Quote => Style::default().fg(Color::DarkGray),
        SpanKind::Link(_) => Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
        SpanKind::Emphasis => Style::default().fg(Color::White).add_modifier(Modifier::ITALIC),
        SpanKind::Strong => Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        SpanKind::Code => Style::default().fg(Color::Green),
    }
}
```

Note: `Wrap { trim: false }` is removed — text is pre-wrapped by the renderer.

**Step 2: Fix call sites in `src/tui/mod.rs`**

The `ensure_preview_loaded` method inserts into cache — update the error branch:

Find the block around line 885-898 in `src/tui/mod.rs`:

```rust
        match mime_render::render_message(&envelope.path, width) {
            Ok(text) => self.preview_cache.insert(msg_id.clone(), width, text),
            Err(e) => self.preview_cache.insert(
                msg_id.clone(),
                width,
                format!("[Error rendering message: {}]", e),
            ),
        }
```

Replace with:

```rust
        match mime_render::render_message(&envelope.path, width) {
            Ok(rendered) => self.preview_cache.insert(msg_id.clone(), width, rendered),
            Err(e) => self.preview_cache.insert(
                msg_id.clone(),
                width,
                mime_render::RenderedMessage {
                    lines: vec![vec![mime_render::RichSpan {
                        text: format!("[Error rendering message: {}]", e),
                        kind: mime_render::SpanKind::Normal,
                    }]],
                    links: Vec::new(),
                },
            ),
        }
```

The preview construction around line 2916 passes `body` to `PreviewPane` — it currently gets `Option<&str>` from `preview_cache.get()`, which now returns `Option<&RenderedMessage>`. The type change should flow through automatically since `PreviewPane::body` now expects `Option<&RenderedMessage>`.

**Step 3: Fix compose call sites in `src/tui/mod.rs`**

Find the three `render_message` calls in the compose section (around lines 1679, 1685, 1691):

```rust
                    mime_render::render_message(&envelope.path, 80).unwrap_or_default();
```

Replace each with:

```rust
                    mime_render::render_message(&envelope.path, 80)
                        .map(|rm| rm.to_plain_text())
                        .unwrap_or_default();
```

**Step 4: Run `cargo check`**

```bash
cargo check
```

Expected: may still have errors from thread_view — that's Task 6.

**Step 5: Commit**

```bash
git add src/tui/preview.rs src/tui/mod.rs
git commit -m "Update PreviewPane to render RenderedMessage with styled spans"
```

---

### Task 6: Update ThreadView widget

**Files:**
- Modify: `src/tui/thread_view.rs`
- Modify: `src/tui/mod.rs` (thread body loading)

**Step 1: Update `ThreadMessage::body` type**

In `src/tui/thread_view.rs`, change:

```rust
pub struct ThreadMessage {
    pub envelope: Envelope,
    pub body: Option<String>,
    pub expanded: bool,
}
```

to:

```rust
use crate::mime_render::{RenderedMessage, SpanKind};

pub struct ThreadMessage {
    pub envelope: Envelope,
    pub body: Option<RenderedMessage>,
    pub expanded: bool,
}
```

**Step 2: Update body rendering in ThreadView::render**

Replace the body rendering block (the `if msg.expanded` section that iterates `body.lines()`) with code that uses `RenderedMessage`. The key change: instead of `body.lines()` producing `&str`, iterate `body.lines` (the `Vec<Vec<RichSpan>>`), and for each span use `span_style()` (import from `preview.rs` or redefine).

Replace the expanded body block:

```rust
            if msg.expanded {
                let wrap_width = area.width.saturating_sub(2) as usize;
                if let Some(ref body) = msg.body {
                    for line in body.lines() {
                        let style = if line.starts_with('>') {
                            header_base.fg(Color::DarkGray)
                        } else {
                            header_base.fg(Color::White)
                        };
                        for wrapped in wrap_line(line, wrap_width) {
                            lines.push(RenderedLine {
                                content: vec![(wrapped, style)],
                                msg_index: Some(idx),
                            });
                        }
                    }
```

with:

```rust
            if msg.expanded {
                if let Some(ref body) = msg.body {
                    for rich_line in &body.lines {
                        let content: Vec<(String, Style)> = rich_line.iter().map(|span| {
                            let style = match &span.kind {
                                SpanKind::Quote => header_base.fg(Color::DarkGray),
                                SpanKind::Link(_) => header_base.fg(Color::Cyan)
                                    .add_modifier(Modifier::UNDERLINED),
                                SpanKind::Emphasis => header_base.fg(Color::White)
                                    .add_modifier(Modifier::ITALIC),
                                SpanKind::Strong => header_base.fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                                SpanKind::Code => header_base.fg(Color::Green),
                                SpanKind::Normal => header_base.fg(Color::White),
                            };
                            (span.text.clone(), style)
                        }).collect();
                        lines.push(RenderedLine {
                            content,
                            msg_index: Some(idx),
                        });
                    }
```

Note: no more `wrap_line` call — text is pre-wrapped by the renderer.

**Step 3: Fix thread body loading in `src/tui/mod.rs`**

Find `ensure_thread_body_loaded` (around line 1591):

```rust
                match mime_render::render_message(&msg.envelope.path, width) {
                    Ok(text) => msg.body = Some(text),
                    Err(e) => msg.body = Some(format!("[Error: {}]", e)),
                }
```

Replace with:

```rust
                match mime_render::render_message(&msg.envelope.path, width) {
                    Ok(rendered) => msg.body = Some(rendered),
                    Err(e) => msg.body = Some(mime_render::RenderedMessage {
                        lines: vec![vec![mime_render::RichSpan {
                            text: format!("[Error: {}]", e),
                            kind: mime_render::SpanKind::Normal,
                        }]],
                        links: Vec::new(),
                    }),
                }
```

**Step 4: Remove `wrap_line` and `truncate_str` from thread_view.rs if no longer used**

Check if `wrap_line` and `truncate_str` are used anywhere else in the file. `truncate_str` is likely still used for header rendering. Keep it if so, remove `wrap_line` if unused.

**Step 5: Run `cargo check` and `cargo test`**

```bash
cargo check && cargo test
```

Expected: compiles and all tests pass.

**Step 6: Commit**

```bash
git add src/tui/thread_view.rs src/tui/mod.rs
git commit -m "Update ThreadView to render RenderedMessage with styled spans"
```

---

### Task 7: Mouse click link handling in preview pane

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/links.rs` (make `open_path` pub)

**Step 1: Make `open_path` public in links.rs**

In `src/links.rs`, change:

```rust
fn open_path(target: &str) -> Result<()> {
```

to:

```rust
pub fn open_path(target: &str) -> Result<()> {
```

**Step 2: Add link click handling to the mouse event handler in `src/tui/mod.rs`**

In the mouse handling section of the event loop, there's already code that handles clicks in the content area (checking `in_content`, `on_tab_bar`, etc.). Add a new branch for clicks in the preview area.

Find the mouse handling section that checks `MouseEventKind::Down(MouseButton::Left) if in_content`. Add after the border drag handling, before the end of the mouse block:

```rust
                    // Link click in preview pane
                    MouseEventKind::Down(MouseButton::Left) if in_content && mouse.column > border_col => {
                        // Check if click is on a link in the preview
                        let preview_x = border_col + 2; // left border + padding
                        let header_lines = 5u16; // Subject, From, To, Date, separator
                        if let Some(envelope) = app.preview_envelope() {
                            if let Some(rendered) = app.preview_cache.get(&envelope.message_id, preview_width) {
                                let content_row = (mouse.row - 1) + app.preview_scroll; // -1 for tab bar
                                if content_row >= header_lines {
                                    let body_line = (content_row - header_lines) as usize;
                                    let col = mouse.column.saturating_sub(preview_x) as usize;
                                    if let Some(link) = rendered.links.iter().find(|l| {
                                        l.line == body_line && col >= l.col_start && col < l.col_end
                                    }) {
                                        let url = link.url.clone();
                                        if url.starts_with("http://") || url.starts_with("https://") {
                                            let _ = links::open_path(&url);
                                            app.set_status(format!("Opened: {}", url));
                                        } else if url.starts_with("mailto:") {
                                            if let Some(parsed) = links::parse_url(&url) {
                                                let _ = app.handle_ipc_command(
                                                    links::IpcCommand::Open(parsed.into())
                                                ).await;
                                            }
                                        } else if url.starts_with("mid:") {
                                            if let Some(parsed) = links::parse_url(&url) {
                                                let _ = app.handle_ipc_command(
                                                    links::IpcCommand::Open(parsed.into())
                                                ).await;
                                            }
                                        } else {
                                            let _ = links::open_path(&url);
                                            app.set_status(format!("Opened: {}", url));
                                        }
                                    }
                                }
                            }
                        }
                    }
```

Note: the exact row/column math may need adjustment based on the actual layout. The preview area starts after the border column, content starts at row 1 (after tab bar). Header is 5 lines (Subject, From, To, Date, blank separator). The `preview_width` variable is already computed in the render loop.

**Step 3: Run `cargo check`**

```bash
cargo check
```

Expected: compiles. There may be borrow checker issues with `app` — the `preview_envelope()` borrow and `preview_cache.get()` borrow need to not conflict. If needed, clone the message_id first.

**Step 4: Commit**

```bash
git add src/tui/mod.rs src/links.rs
git commit -m "Add mouse click handling for links in preview pane"
```

---

### Task 8: Parse mid: content-id suffix (RFC 2392)

**Files:**
- Modify: `src/links.rs`

**Step 1: Write tests for mid: with content-id**

Add to the test module in `src/links.rs`:

```rust
    #[test]
    fn parse_mid_with_content_id() {
        assert_eq!(
            parse_url("mid:abc@example.com/1.3"),
            Some(HuttUrl::MessagePart {
                message_id: "abc@example.com".to_string(),
                content_id: "1.3".to_string(),
                account: None,
            })
        );
    }

    #[test]
    fn parse_mid_with_content_id_and_account() {
        assert_eq!(
            parse_url("mid:abc@example.com/1.3?account=work"),
            Some(HuttUrl::MessagePart {
                message_id: "abc@example.com".to_string(),
                content_id: "1.3".to_string(),
                account: Some("work".to_string()),
            })
        );
    }

    #[test]
    fn parse_mid_without_content_id_unchanged() {
        // Existing behavior: no content-id → Message variant
        assert!(matches!(
            parse_url("mid:abc@example.com"),
            Some(HuttUrl::Message { .. })
        ));
    }
```

**Step 2: Run tests to verify they fail**

```bash
cargo test --lib links::tests::parse_mid_with_content 2>&1 | head -10
```

Expected: FAIL — `HuttUrl::MessagePart` does not exist.

**Step 3: Add `MessagePart` variant to `HuttUrl` and update parser**

Add to the `HuttUrl` enum:

```rust
    MessagePart {
        message_id: String,
        content_id: String,
        account: Option<String>,
    },
```

Also add the same variant to `HuttUrlSerde` (the serde-friendly version) and update the `From` impls.

Update the `mid:` parsing in `parse_url()`:

```rust
    // mid:<message-id>[/<content-id>][?view=thread]
    if let Some(rest) = url.strip_prefix("mid:") {
        let (id_part, qs) = split_query(rest);
        if id_part.is_empty() {
            return None;
        }
        let params = parse_query_string(qs);
        let account = params.get("account").cloned();

        // Check for content-id: mid:message-id/content-id
        if let Some(slash_pos) = id_part.find('/') {
            let message_id = id_part[..slash_pos].to_string();
            let content_id = id_part[slash_pos + 1..].to_string();
            if !message_id.is_empty() && !content_id.is_empty() {
                return Some(HuttUrl::MessagePart { message_id, content_id, account });
            }
        }

        let id = url_decode(id_part);
        if params.get("view").map(|v| v.as_str()) == Some("thread") {
            return Some(HuttUrl::Thread { id: id.to_string(), account });
        }
        return Some(HuttUrl::Message { id: id.to_string(), account });
    }
```

**Step 4: Run tests**

```bash
cargo test --lib links::tests
```

Expected: all PASS.

**Step 5: Commit**

```bash
git add src/links.rs
git commit -m "Parse mid: content-id suffix per RFC 2392"
```

---

### Task 9: Attachment extraction

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Write test for attachment extraction**

Add to the test module:

```rust
    #[test]
    fn extract_attachment_by_index() {
        // Build a minimal multipart message with an attachment
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
```

**Step 2: Run test to verify it fails**

```bash
cargo test --lib mime_render::tests::extract_attachment 2>&1 | head -10
```

Expected: FAIL — `extract_attachment_from_bytes` does not exist.

**Step 3: Implement attachment extraction**

Add to `src/mime_render.rs`:

```rust
/// An extracted attachment ready to save or open.
pub struct ExtractedAttachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Extract an attachment from a message file by content-id.
/// Content-id can be a MIME Content-ID header value or "part.N" (1-indexed part number).
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
                return extract_part(&message, part, idx);
            }
        }
    }

    // Try matching by positional index: "part.N"
    if let Some(n_str) = content_id.strip_prefix("part.") {
        if let Ok(n) = n_str.parse::<usize>() {
            if let Some(part) = message.parts.get(n) {
                return extract_part(&message, part, n);
            }
        }
    }

    anyhow::bail!("attachment not found: {}", content_id)
}

fn extract_part(
    _message: &mail_parser::Message,
    part: &mail_parser::MessagePart,
    idx: usize,
) -> Result<ExtractedAttachment> {
    let filename = part.attachment_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("attachment-{}", idx));

    let mime_type = part.content_type()
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
            // For nested messages, serialize back — but raw bytes aren't easily available,
            // so just return an empty vec with a note
            format!("Nested message: {}", msg.subject().unwrap_or("(no subject)")).into_bytes()
        }
        mail_parser::PartType::Multipart(_) => {
            anyhow::bail!("cannot extract multipart container as attachment");
        }
    };

    Ok(ExtractedAttachment { filename, mime_type, data })
}
```

**Step 4: Run tests**

```bash
cargo test --lib mime_render::tests
```

Expected: all PASS.

**Step 5: Commit**

```bash
git add src/mime_render.rs
git commit -m "Add attachment extraction by content-id or part index"
```

---

### Task 10: Attachment list in rendered output

**Files:**
- Modify: `src/mime_render.rs`

**Step 1: Write test for attachment list rendering**

Add to test module:

```rust
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
        // Should have a link for the attachment
        let att_links: Vec<_> = rm.links.iter()
            .filter(|l| l.url.starts_with("mid:"))
            .collect();
        assert!(!att_links.is_empty());
    }
```

**Step 2: Run test to verify it fails**

```bash
cargo test --lib mime_render::tests::render_message_with_attach 2>&1 | head -10
```

Expected: FAIL — `render_message_from_bytes` doesn't exist.

**Step 3: Implement attachment list rendering**

Add a new function `render_message_from_bytes` that `render_message` delegates to (so we can test without filesystem), and add attachment discovery:

```rust
/// Render from raw bytes (testable without filesystem).
pub fn render_message_from_bytes(raw: &[u8], message_id: &str, width: u16) -> Result<RenderedMessage> {
    let message = mail_parser::MessageParser::default()
        .parse(raw)
        .context("failed to parse MIME message")?;

    // Render body
    let mut rendered = if let Some(text) = message.body_text(0) {
        render_plain_text(&text, width)
    } else if let Some(html) = message.body_html(0) {
        render_html(html.as_bytes(), width)
    } else {
        // Check nested parts
        let mut found = None;
        for part in message.parts.iter() {
            match &part.body {
                mail_parser::PartType::Text(text) if part.is_content_type("text", "plain") => {
                    found = Some(render_plain_text(text, width));
                    break;
                }
                _ => {}
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
            lines: vec![vec![RichSpan { text: "[No text content]".to_string(), kind: SpanKind::Normal }]],
            links: Vec::new(),
        })
    };

    // Discover attachments
    let attachments = discover_attachments(&message);
    if !attachments.is_empty() {
        append_attachment_list(&mut rendered, &attachments, message_id, width);
    }

    Ok(rendered)
}

struct AttachmentInfo {
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
    pub content_id: String, // Content-ID header or "part.N"
}

fn discover_attachments(message: &mail_parser::Message) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();

    for (idx, part) in message.parts.iter().enumerate() {
        // Skip the root multipart container and text body parts
        match &part.body {
            mail_parser::PartType::Multipart(_) => continue,
            mail_parser::PartType::Text(_) if part.is_content_type("text", "plain") => {
                // Skip if this is a body part (index 0 or 1 typically)
                if idx <= 1 { continue; }
                // If it has a filename, treat as attachment
                if part.attachment_name().is_none() { continue; }
            }
            mail_parser::PartType::Html(_) => {
                if idx <= 1 { continue; }
                if part.attachment_name().is_none() { continue; }
            }
            _ => {}
        }

        let filename = part.attachment_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("attachment-{}", idx));

        let mime_type = part.content_type()
            .map(|ct| {
                if let Some(subtype) = ct.subtype() {
                    format!("{}/{}", ct.ctype(), subtype)
                } else {
                    ct.ctype().to_string()
                }
            })
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let size = match &part.body {
            mail_parser::PartType::Binary(cow) | mail_parser::PartType::InlineBinary(cow) => cow.len(),
            mail_parser::PartType::Text(cow) => cow.len(),
            mail_parser::PartType::Html(cow) => cow.len(),
            _ => 0,
        };

        let content_id = part.content_id()
            .map(|cid| cid.trim_matches(|c| c == '<' || c == '>').to_string())
            .unwrap_or_else(|| format!("part.{}", idx));

        attachments.push(AttachmentInfo { filename, mime_type, size, content_id });
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
    // Blank line + separator
    rendered.lines.push(Vec::new());
    let sep_width = (width as usize).min(50);
    let separator = format!("── Attachments {}", "─".repeat(sep_width.saturating_sub(15)));
    rendered.lines.push(vec![RichSpan { text: separator, kind: SpanKind::Normal }]);

    for att in attachments {
        let line_idx = rendered.lines.len();
        let url = format!("mid:{}/{}", message_id, att.content_id);
        let label = format!("📎 {} ({}, {})", att.filename, att.mime_type, format_size(att.size));
        let col_start = 0;
        let col_end = label.chars().count();

        rendered.links.push(LinkRegion {
            line: line_idx,
            col_start,
            col_end,
            url: url.clone(),
        });

        rendered.lines.push(vec![RichSpan {
            text: label,
            kind: SpanKind::Link(url),
        }]);
    }
}
```

**Step 4: Update `render_message` to delegate to `render_message_from_bytes`**

```rust
pub fn render_message(path: &Path, width: u16) -> Result<RenderedMessage> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading message file: {}", path.display()))?;

    let message = mail_parser::MessageParser::default()
        .parse(&raw)
        .context("failed to parse MIME message")?;

    let message_id = message.message_id().unwrap_or("unknown").to_string();

    render_message_from_bytes(&raw, &message_id, width)
}
```

**Step 5: Run tests**

```bash
cargo test --lib mime_render::tests
```

Expected: all PASS.

**Step 6: Commit**

```bash
git add src/mime_render.rs
git commit -m "Add attachment list to rendered message output"
```

---

### Task 11: Handle attachment clicks (open with OS handler)

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Add attachment open handler**

In the `handle_ipc_command` method (or in the mouse click handler from Task 7), add handling for `HuttUrl::MessagePart`:

When a `mid:` URL with content-id is clicked, look up the message by message-id in the current view, extract the attachment, write to temp file, and open:

```rust
// In the link click dispatch (from Task 7), add handling for mid: URLs
// that resolve to MessagePart:
if url.starts_with("mid:") {
    if let Some(parsed) = links::parse_url(&url) {
        match parsed {
            HuttUrl::MessagePart { message_id, content_id, .. } => {
                // Find the message path
                if let Some(path) = app.find_message_path(&message_id) {
                    match mime_render::extract_attachment(&path, &content_id) {
                        Ok(att) => {
                            let tmp_dir = std::env::temp_dir();
                            let tmp_path = tmp_dir.join(&att.filename);
                            if let Err(e) = std::fs::write(&tmp_path, &att.data) {
                                app.set_status(format!("Write error: {}", e));
                            } else {
                                let _ = links::open_path(
                                    tmp_path.to_str().unwrap_or("")
                                );
                                app.set_status(format!("Opened: {}", att.filename));
                            }
                        }
                        Err(e) => app.set_status(format!("Extract error: {}", e)),
                    }
                } else {
                    app.set_status("Message not found");
                }
            }
            _ => {
                let _ = app.handle_ipc_command(
                    links::IpcCommand::Open(parsed.into())
                ).await;
            }
        }
    }
}
```

**Step 2: Add `find_message_path` helper to App**

Add a method that searches the current envelope list and thread messages for a message-id and returns its filesystem path:

```rust
    fn find_message_path(&self, message_id: &str) -> Option<std::path::PathBuf> {
        // Check current envelope list
        for e in &self.envelopes {
            if e.message_id == message_id {
                return Some(std::path::PathBuf::from(&e.path));
            }
        }
        // Check thread messages
        for msg in &self.thread_messages {
            if msg.envelope.message_id == message_id {
                return Some(std::path::PathBuf::from(&msg.envelope.path));
            }
        }
        None
    }
```

**Step 3: Run `cargo check` and `cargo test`**

```bash
cargo check && cargo test
```

Expected: compiles and tests pass.

**Step 4: Commit**

```bash
git add src/tui/mod.rs
git commit -m "Handle attachment clicks: extract and open with OS handler"
```

---

### Task 12: Thread view link clicks

**Files:**
- Modify: `src/tui/mod.rs`

**Step 1: Add link click handling for thread view**

In the thread view, the rendered content is managed differently — it uses direct buffer rendering with a scroll offset. Add mouse click handling similar to preview but accounting for thread view layout.

In the mouse event handler, when `app.mode == InputMode::ThreadView`:

```rust
                    MouseEventKind::Down(MouseButton::Left) if in_content && app.mode == InputMode::ThreadView => {
                        // Hit-test links in thread view
                        let content_row = (mouse.row.saturating_sub(1) + app.thread_scroll) as usize;
                        let col = mouse.column.saturating_sub(1) as usize; // 1 char left padding

                        // Find which message this row belongs to and its body offset
                        let mut row_counter = 2usize; // header + blank line
                        for (idx, msg) in app.thread_messages.iter().enumerate() {
                            if idx > 0 { row_counter += 1; } // separator
                            row_counter += 1; // message header line

                            if msg.expanded {
                                if let Some(ref body) = msg.body {
                                    let body_start = row_counter;
                                    let body_end = body_start + body.lines.len();
                                    if content_row >= body_start && content_row < body_end {
                                        let body_line = content_row - body_start;
                                        if let Some(link) = body.links.iter().find(|l| {
                                            l.line == body_line && col >= l.col_start && col < l.col_end
                                        }) {
                                            let url = link.url.clone();
                                            // Dispatch URL (same logic as preview)
                                            if url.starts_with("http://") || url.starts_with("https://") {
                                                let _ = links::open_path(&url);
                                                app.set_status(format!("Opened: {}", url));
                                            } else if url.starts_with("mid:") {
                                                if let Some(parsed) = links::parse_url(&url) {
                                                    match parsed {
                                                        links::HuttUrl::MessagePart { ref message_id, ref content_id, .. } => {
                                                            if let Some(path) = app.find_message_path(message_id) {
                                                                match mime_render::extract_attachment(&path, content_id) {
                                                                    Ok(att) => {
                                                                        let tmp_path = std::env::temp_dir().join(&att.filename);
                                                                        let _ = std::fs::write(&tmp_path, &att.data);
                                                                        let _ = links::open_path(tmp_path.to_str().unwrap_or(""));
                                                                        app.set_status(format!("Opened: {}", att.filename));
                                                                    }
                                                                    Err(e) => app.set_status(format!("Extract error: {}", e)),
                                                                }
                                                            }
                                                        }
                                                        _ => {
                                                            let _ = app.handle_ipc_command(
                                                                links::IpcCommand::Open(parsed.into())
                                                            ).await;
                                                        }
                                                    }
                                                }
                                            } else {
                                                let _ = links::open_path(&url);
                                                app.set_status(format!("Opened: {}", url));
                                            }
                                            break;
                                        }
                                    }
                                    row_counter = body_end + 1; // +1 blank line after body
                                } else {
                                    row_counter += 2; // "Loading…" + blank line
                                }
                            }
                        }
                    }
```

Note: this duplicates the URL dispatch logic from Task 7. Consider extracting a `dispatch_link_url` method on App to DRY this up.

**Step 2: Extract `dispatch_link_url` helper**

```rust
    async fn dispatch_link_url(&mut self, url: &str) {
        if url.starts_with("http://") || url.starts_with("https://") {
            let _ = links::open_path(url);
            self.set_status(format!("Opened: {}", url));
        } else if url.starts_with("mid:") {
            if let Some(parsed) = links::parse_url(url) {
                match parsed {
                    HuttUrl::MessagePart { ref message_id, ref content_id, .. } => {
                        if let Some(path) = self.find_message_path(message_id) {
                            match mime_render::extract_attachment(&path, content_id) {
                                Ok(att) => {
                                    let tmp_path = std::env::temp_dir().join(&att.filename);
                                    match std::fs::write(&tmp_path, &att.data) {
                                        Ok(_) => {
                                            let _ = links::open_path(
                                                tmp_path.to_str().unwrap_or("")
                                            );
                                            self.set_status(format!("Opened: {}", att.filename));
                                        }
                                        Err(e) => self.set_status(format!("Write error: {}", e)),
                                    }
                                }
                                Err(e) => self.set_status(format!("Extract error: {}", e)),
                            }
                        } else {
                            self.set_status("Message not found");
                        }
                    }
                    _ => {
                        let _ = self.handle_ipc_command(
                            links::IpcCommand::Open(parsed.into())
                        ).await;
                    }
                }
            }
        } else if url.starts_with("mailto:") {
            if let Some(parsed) = links::parse_url(url) {
                let _ = self.handle_ipc_command(
                    links::IpcCommand::Open(parsed.into())
                ).await;
            }
        } else {
            let _ = links::open_path(url);
            self.set_status(format!("Opened: {}", url));
        }
    }
```

Then both preview and thread view link clicks just call `app.dispatch_link_url(&url).await`.

**Step 3: Run `cargo check` and `cargo test`**

```bash
cargo check && cargo test
```

Expected: compiles and tests pass.

**Step 4: Commit**

```bash
git add src/tui/mod.rs
git commit -m "Add link click handling in thread view, extract dispatch_link_url helper"
```

---

### Task 13: Pass message_id into render_message for attachment links

**Files:**
- Modify: `src/mime_render.rs`
- Modify: `src/tui/mod.rs`

The current `render_message(path, width)` signature doesn't include `message_id`, but the attachment list needs it for `mid:` URLs. Two options:
1. Parse the message-id from the file inside `render_message` (already done in Task 10)
2. Pass it in from the caller (callers already have it)

Task 10 already handles this — `render_message` reads the file, parses the message-id, and delegates to `render_message_from_bytes`. But we should also update `ensure_preview_loaded` and `ensure_thread_body_loaded` to pass the message-id so we don't parse the file twice.

**Step 1: Update render_message signature to accept message_id**

Change to accept an optional message_id hint:

```rust
pub fn render_message(path: &Path, message_id: &str, width: u16) -> Result<RenderedMessage> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading message file: {}", path.display()))?;
    render_message_from_bytes(&raw, message_id, width)
}
```

**Step 2: Update all call sites to pass message_id**

In `ensure_preview_loaded`:
```rust
        match mime_render::render_message(&envelope.path, msg_id, width) {
```

In `ensure_thread_body_loaded`:
```rust
                match mime_render::render_message(&msg.envelope.path, &msg.envelope.message_id, width) {
```

In compose (the three `render_message` calls) — these don't need attachments, but the signature now requires message_id. Pass the envelope's message_id:
```rust
                    mime_render::render_message(&envelope.path, &envelope.message_id, 80)
                        .map(|rm| rm.to_plain_text())
                        .unwrap_or_default();
```

**Step 3: Run `cargo check` and `cargo test`**

```bash
cargo check && cargo test
```

**Step 4: Commit**

```bash
git add src/mime_render.rs src/tui/mod.rs
git commit -m "Pass message_id to render_message for attachment mid: URLs"
```

---

### Task 14: download_dir config option

**Files:**
- Modify: `src/config.rs`
- Modify: `config.sample.toml`

**Step 1: Add `download_dir` to Config**

```rust
    /// Directory to save attachments to. Default: ~/Downloads.
    pub download_dir: Option<String>,
```

Add to the `Default` impl too:
```rust
            download_dir: None,
```

**Step 2: Update config.sample.toml**

Add after the sync_command section:

```toml
# Directory for saved attachments. Default: ~/Downloads
# download_dir = "~/Downloads"
```

**Step 3: Run `cargo check` and `cargo test`**

```bash
cargo check && cargo test
```

**Step 4: Commit**

```bash
git add src/config.rs config.sample.toml
git commit -m "Add download_dir config option for attachment saving"
```

---

### Task 15: Final integration test and cleanup

**Files:**
- All modified files

**Step 1: Run full test suite**

```bash
cargo test
```

Expected: all tests pass.

**Step 2: Run clippy**

```bash
cargo clippy -- -W clippy::all
```

Expected: no warnings (or only pre-existing ones).

**Step 3: Test with a real email (manual)**

```bash
HUTT_LOG=/tmp/hutt.log cargo run --release
```

- Open an HTML email with links — verify links render in cyan+underline
- Click a link — verify it opens in browser
- Open an email with attachments — verify attachment list appears
- Click an attachment — verify it opens with OS handler
- Open thread view — verify links and attachments work there too
- Check /tmp/hutt.log for any errors

**Step 4: Commit any final fixes**

```bash
git add -A
git commit -m "Clickable links and attachment handling: final polish"
```

---

## Summary of Tasks

| Task | Description | Key files |
|------|-------------|-----------|
| 1 | RenderedMessage data types | mime_render.rs |
| 2 | Plain text rendering with URL detection | mime_render.rs |
| 3 | HTML rendering with rich annotations | mime_render.rs |
| 4 | Update render_message + RenderCache | mime_render.rs |
| 5 | Update PreviewPane widget | preview.rs, mod.rs |
| 6 | Update ThreadView widget | thread_view.rs, mod.rs |
| 7 | Mouse click link handling (preview) | mod.rs, links.rs |
| 8 | Parse mid: content-id (RFC 2392) | links.rs |
| 9 | Attachment extraction | mime_render.rs |
| 10 | Attachment list in rendered output | mime_render.rs |
| 11 | Handle attachment clicks | mod.rs |
| 12 | Thread view link clicks + dispatch_link_url | mod.rs |
| 13 | Pass message_id to render_message | mime_render.rs, mod.rs |
| 14 | download_dir config option | config.rs, config.sample.toml |
| 15 | Integration test and cleanup | all |
