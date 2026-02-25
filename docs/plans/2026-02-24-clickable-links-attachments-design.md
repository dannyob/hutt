# Clickable Links and Attachment Handling

## Overview

Two features that build on each other:

1. **Step 0 — Clickable links in rendered email**: Track link regions in rendered message content, style them visually, handle mouse clicks to open URLs.
2. **Step 1 — Attachment display and handling**: Show attachments as a list with clickable `mid:` URLs (RFC 2392 content-id form), extract and open/save attachments on click.

A future Step 2 (inline image rendering via ratatui-image) is out of scope but the design accommodates it.

## Step 0: Clickable Links

### RenderedMessage abstraction

`mime_render::render_message()` currently returns `Result<String>`. It changes to return `Result<RenderedMessage>`:

```rust
pub struct RenderedMessage {
    /// Pre-wrapped lines, each a sequence of styled spans.
    pub lines: Vec<Vec<RichSpan>>,
    /// Clickable link regions for mouse hit-testing.
    pub links: Vec<LinkRegion>,
}

pub struct RichSpan {
    pub text: String,
    pub kind: SpanKind,
}

pub enum SpanKind {
    Normal,
    Quote,          // lines starting with >
    Link(String),   // clickable — carries target URL
    Emphasis,
    Strong,
    Code,
}

pub struct LinkRegion {
    pub line: usize,      // index into RenderedMessage::lines
    pub col_start: usize, // character offset (not byte)
    pub col_end: usize,
    pub url: String,
}
```

### Rendering pipeline

**HTML emails**: Use `html2text::from_read_rich()` instead of `from_read()`. This returns `Vec<TaggedLine<Vec<RichAnnotation>>>` where each span carries annotations like `RichAnnotation::Link(url)`, `Image(src)`, `Emphasis`, `Strong`, `Code`, etc. Map these to `SpanKind` variants. Build `LinkRegion` entries from the `Link` spans' positions.

**Plain text emails**: Split into lines, scan each line for URLs using a simple pattern match (`http://`, `https://`, `mid:`, `mailto:`). Wrap matched ranges in `SpanKind::Link`. Lines starting with `>` get `SpanKind::Quote`.

**Pre-wrapping**: The renderer word-wraps all lines to the terminal width (already known — it's the cache key). This ensures each `RenderedMessage` line maps 1:1 to a screen row, making mouse hit-testing straightforward. Link regions that span a wrap boundary get split into multiple `LinkRegion` entries.

### RenderCache

Changes from `HashMap<(String, u16), String>` to `HashMap<(String, u16), RenderedMessage>`.

### Widget changes

**PreviewPane**: Accepts `Option<&RenderedMessage>` instead of `Option<&str>`. Maps `SpanKind` to ratatui styles:
- `Normal` → white
- `Quote` → dark gray
- `Link` → cyan + underline
- `Emphasis` → italic
- `Strong` → bold
- `Code` → green (or similar)

Removes `Wrap { trim: false }` from `Paragraph` since text is pre-wrapped. Scroll unchanged.

**ThreadView**: `ThreadMessage::body` changes from `Option<String>` to `Option<RenderedMessage>`. Same style mapping. Thread view already does its own wrapping via `wrap_line()` — this moves into the renderer.

### Mouse click handling

On `MouseEventKind::Down` in the preview/thread content area:

1. Compute content line: `content_line = (screen_row - content_area.y) + scroll_offset`
2. Compute content column: `col = screen_col - content_area.x - padding`
3. Look up in the active `RenderedMessage::links` for any `LinkRegion` where `line == content_line && col_start <= col && col < col_end`
4. If found, dispatch the URL:
   - `http://` / `https://` → `open_path()` (open/xdg-open)
   - `mid:` with content-id → extract and open/save attachment (Step 1)
   - `mid:` without content-id → `handle_ipc_command` / navigate
   - `mailto:` → compose

For thread view, add an offset for each message body's starting line within the overall thread layout.

### Visual feedback

Link text rendered in cyan with underline. No hover state needed (terminal mouse doesn't have hover in most protocols, and even where it does, the redraw cost isn't worth it for now).

## Step 1: Attachment Display and Handling

### Attachment list in rendered output

After the message body, `render_message()` appends an attachment section:

```
── Attachments ────────────────────────────────────
📎 report.pdf (application/pdf, 245 KB)     mid:abc@example.com/1.3
📎 photo.jpg (image/jpeg, 1.2 MB)           mid:abc@example.com/1.4
📎 data.csv (text/csv, 12 KB)               mid:abc@example.com/1.5
```

Each attachment line is a `SpanKind::Link` pointing to a `mid:` URL with RFC 2392 content-id form: `mid:<message-id>/<content-id>`.

Attachments are discovered by iterating `mail_parser::Message::parts` and collecting non-body parts that have a filename or a content-type that isn't `text/plain` or `text/html` (the body parts already rendered above).

The attachment info needed per part:
- Filename (from Content-Disposition or Content-Type `name` parameter)
- MIME type
- Size (byte length of decoded content)
- Content-ID (from Content-ID header, or synthesized as positional index like `part.N` if missing)

### mid: URL with content-id

RFC 2392 defines: `mid-url = "mid:" message-id [ "/" content-id ]`

Extend `parse_url()` in `links.rs` to parse the `/content-id` suffix:

```rust
// Current: HuttUrl::Message { id, account }
// New variant or extended:
HuttUrl::MessagePart { message_id: String, content_id: String, account: Option<String> }
```

When `mid:abc@example.com/1.3` is parsed, it becomes `HuttUrl::MessagePart { message_id: "abc@example.com", content_id: "1.3", .. }`.

### Attachment extraction

New function in `mime_render.rs` (or a new `attachments.rs`):

```rust
pub fn extract_attachment(message_path: &Path, content_id: &str) -> Result<ExtractedAttachment>

pub struct ExtractedAttachment {
    pub filename: String,      // from Content-Disposition or derived
    pub mime_type: String,
    pub data: Vec<u8>,         // decoded content
}
```

Finds the MIME part matching the content-id (either by Content-ID header or positional index), decodes it, and returns the raw bytes.

### Open vs Save

When a `mid:` URL with content-id is clicked (or activated via `hutt remote open-url`):

**In TUI**: Write to a temp file and open with `open`/`xdg-open`. Simple, immediate. The OS picks the right application.

**Download directory**: Config option `download_dir` (default: `~/Downloads`). When explicitly "saving" (future keybinding, e.g. `d` on a selected attachment), write to `download_dir/filename` instead of temp.

For now, clicking always opens (temp file + OS handler). A save action can be added later.

### Config

```toml
# Directory for saved attachments (default: ~/Downloads)
# download_dir = "~/Downloads"
```

## What this does NOT cover

- **Inline image rendering** (Step 2, future): ratatui-image integration for rendering images inline in the preview/thread view. The `SpanKind` enum and `RenderedMessage` structure accommodate this — a future `SpanKind::Image { path, alt_text }` variant could trigger image widget rendering.
- **Attachment save keybinding**: Click-to-open is step 1. An explicit save-to-disk action (e.g. `d` key on attachment line, or a download picker) can follow.
- **Hover states**: No mouse hover highlighting on links. Terminal mouse support for hover is spotty and the redraw overhead isn't worth it.

## Files to modify

- `src/mime_render.rs` — `RenderedMessage`, `RichSpan`, `SpanKind`, `LinkRegion`, `render_message()` rewrite, attachment extraction
- `src/links.rs` — parse `mid:` content-id suffix, `HuttUrl::MessagePart` variant
- `src/tui/preview.rs` — accept `&RenderedMessage`, style mapping, remove `Wrap`
- `src/tui/thread_view.rs` — accept `&RenderedMessage` per message, style mapping
- `src/tui/mod.rs` — mouse click hit-testing for links, attachment open handler, cache type change
- `src/config.rs` — `download_dir` option
- `Cargo.toml` — no new dependencies (html2text rich mode is already available)
