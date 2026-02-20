use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::envelope::Envelope;
use crate::links;

pub struct PreviewPane<'a> {
    pub envelope: Option<&'a Envelope>,
    pub body: Option<&'a str>,
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
                    envelope
                        .from
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    value_style,
                ),
            ]),
            Line::from(vec![
                Span::styled("To:      ", header_style),
                Span::styled(
                    envelope
                        .to
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
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

        // Add body lines
        if let Some(body) = self.body {
            for line in body.lines() {
                let style = if line.starts_with('>') {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(Span::styled(line.to_string(), style)));
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
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));

        paragraph.render(area, buf);
    }
}

/// A region of text that should be an OSC 8 hyperlink.
pub struct HyperlinkRegion {
    pub url: String,
    pub text: String,
    pub x: u16,
    pub y: u16,
    /// ANSI SGR codes to restore the style when re-emitting the text.
    pub sgr: String,
}

/// Compute hyperlink regions for the preview pane headers.
///
/// Call this with the same area passed to `PreviewPane::render()` and
/// then write the returned regions to the terminal after the frame is
/// flushed.
pub fn preview_hyperlinks(
    envelope: &Envelope,
    area: Rect,
    scroll: u16,
) -> Vec<HyperlinkRegion> {
    let mut regions = Vec::new();

    // Content starts 1 col past the left border
    let content_x = area.x + 1;
    let label_width = 9u16; // "Subject: " / "From:    "
    let value_x = content_x + label_width;
    let max_w = area.width.saturating_sub(1 + label_width);

    // Row 0 (Subject) → hutt://thread/MESSAGE_ID
    // Style: bold white  →  SGR: \x1b[1;37m
    if scroll == 0 && !envelope.message_id.is_empty() {
        let url = links::format_thread_url(&envelope.message_id);
        let max_chars = max_w as usize;
        let text: String = envelope.subject.chars().take(max_chars).collect();
        regions.push(HyperlinkRegion {
            url,
            text,
            x: value_x,
            y: area.y,
            sgr: "\x1b[1;37m".to_string(), // bold + white
        });
    }

    // Row 1 (From) → hutt://search/from:EMAIL for each address
    // Style: white  →  SGR: \x1b[37m
    if scroll <= 1 {
        let from_y = area.y + 1 - scroll;
        let mut col = value_x;
        for (i, addr) in envelope.from.iter().enumerate() {
            let display = addr.to_string();
            let display_w = display.len() as u16;
            let url = links::format_search_url(&format!("from:{}", addr.email));
            let avail = max_w.saturating_sub(col - value_x) as usize;
            let text: String = display.chars().take(avail).collect();
            if !text.is_empty() {
                regions.push(HyperlinkRegion {
                    url,
                    text,
                    x: col,
                    y: from_y,
                    sgr: "\x1b[37m".to_string(), // white
                });
            }
            col += display_w;
            if i < envelope.from.len() - 1 {
                col += 2; // ", "
            }
        }
    }

    regions
}

/// Scan the rendered buffer for URLs and return hyperlink regions.
///
/// Reads all visible text across rows as a continuous stream so that
/// URLs wrapped across lines are detected as a single URL, then maps
/// each URL back to per-row regions for rendering.
pub fn scan_buffer_urls(buf: &Buffer, area: Rect) -> Vec<HyperlinkRegion> {
    // Content area: 1 col in from left border
    let x_start = area.x + 1;
    let x_end = area.x + area.width;

    // Build a flat stream of characters with their screen coordinates.
    // When a row is full to the edge (soft-wrapped), we don't insert a
    // separator.  Otherwise we insert a space so URL detection stops at
    // hard line breaks.
    struct CharPos {
        x: u16,
        y: u16,
    }
    let mut full_text = String::new();
    let mut positions: Vec<CharPos> = Vec::new();

    // positions is byte-indexed (one entry per byte of full_text) so that
    // byte offsets from String::find() can index directly into it.
    for y in area.y..area.y + area.height {
        let mut x = x_start;
        let mut row_end_x = x_start;
        while x < x_end {
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            for ch in sym.chars() {
                let pos = CharPos { x, y };
                for _ in 0..ch.len_utf8() {
                    positions.push(CharPos { x: pos.x, y: pos.y });
                }
                full_text.push(ch);
            }
            let w = (unicode_width::UnicodeWidthStr::width(sym)).max(1) as u16;
            row_end_x = x + w;
            x += w;
        }
        // If row didn't fill to the edge, insert a space as a word break
        // so URLs don't span across hard line boundaries.
        if row_end_x < x_end {
            positions.push(CharPos { x: x_end, y });
            full_text.push(' ');
        }
    }

    // Find URLs in the continuous text
    let mut regions = Vec::new();
    let mut search_from = 0;
    while search_from < full_text.len() {
        let rest = &full_text[search_from..];
        let url_start = if let Some(pos) = rest.find("https://") {
            pos
        } else if let Some(pos) = rest.find("http://") {
            pos
        } else {
            break;
        };
        let abs_start = search_from + url_start;
        // Extend URL until whitespace or bracket-like delimiter
        let url_end = full_text[abs_start..]
            .find(|c: char| c.is_whitespace() || "<>\"'`|{}[]".contains(c))
            .map(|p| abs_start + p)
            .unwrap_or(full_text.len());
        // Strip trailing punctuation
        let mut end = url_end;
        while end > abs_start {
            let last = full_text.as_bytes()[end - 1];
            if b".,;:!?)>".contains(&last) {
                end -= 1;
            } else {
                break;
            }
        }
        if end > abs_start + 8 {
            let url = &full_text[abs_start..end];
            // Split into per-row regions
            let mut row_start = abs_start;
            while row_start < end {
                let row_y = positions[row_start].y;
                // Find where this row's portion ends
                let mut row_end = row_start;
                while row_end < end && positions[row_end].y == row_y {
                    row_end += 1;
                }
                let text = &full_text[row_start..row_end];
                let screen_x = positions[row_start].x;
                let cell = &buf[(screen_x, row_y)];
                let sgr = cell_sgr(cell);
                regions.push(HyperlinkRegion {
                    url: url.to_string(),
                    text: text.to_string(),
                    x: screen_x,
                    y: row_y,
                    sgr,
                });
                row_start = row_end;
            }
        }
        search_from = end.max(search_from + 1);
    }
    regions
}

/// Convert a ratatui Cell's style to an ANSI SGR escape sequence.
fn cell_sgr(cell: &ratatui::buffer::Cell) -> String {
    let style = cell.style();
    let mut codes = Vec::new();
    if style.add_modifier.contains(Modifier::BOLD) {
        codes.push("1".to_string());
    }
    if style.add_modifier.contains(Modifier::DIM) {
        codes.push("2".to_string());
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        codes.push("3".to_string());
    }
    if style.add_modifier.contains(Modifier::UNDERLINED) {
        codes.push("4".to_string());
    }
    if let Some(fg) = style.fg {
        if let Some(code) = color_to_sgr(fg, false) {
            codes.push(code);
        }
    }
    if let Some(bg) = style.bg {
        if let Some(code) = color_to_sgr(bg, true) {
            codes.push(code);
        }
    }
    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

fn color_to_sgr(color: Color, bg: bool) -> Option<String> {
    let base = if bg { 40 } else { 30 };
    match color {
        Color::Black => Some(format!("{}", base)),
        Color::Red => Some(format!("{}", base + 1)),
        Color::Green => Some(format!("{}", base + 2)),
        Color::Yellow => Some(format!("{}", base + 3)),
        Color::Blue => Some(format!("{}", base + 4)),
        Color::Magenta => Some(format!("{}", base + 5)),
        Color::Cyan => Some(format!("{}", base + 6)),
        Color::White => Some(format!("{}", base + 7)),
        Color::DarkGray => Some(format!("{}", base + 60)),
        Color::LightRed => Some(format!("{}", base + 60 + 1)),
        Color::LightGreen => Some(format!("{}", base + 60 + 2)),
        Color::LightYellow => Some(format!("{}", base + 60 + 3)),
        Color::LightBlue => Some(format!("{}", base + 60 + 4)),
        Color::LightMagenta => Some(format!("{}", base + 60 + 5)),
        Color::LightCyan => Some(format!("{}", base + 60 + 6)),
        Color::Gray => Some(format!("{}", base + 60 + 7)),
        Color::Rgb(r, g, b) => Some(format!("{};2;{};{};{}", base + 8, r, g, b)),
        Color::Indexed(n) => Some(format!("{};5;{}", base + 8, n)),
        _ => None,
    }
}

/// Write OSC 8 hyperlink escape sequences directly to the terminal.
///
/// Re-emits the text at each region's position wrapped in OSC 8
/// markers, with the correct SGR style so the visual output is
/// identical to what ratatui rendered.  All writes are batched
/// into a single flush to avoid flicker.
pub fn write_hyperlinks<W: std::io::Write>(
    w: &mut W,
    regions: &[HyperlinkRegion],
) -> std::io::Result<()> {
    use crossterm::cursor::MoveTo;
    use crossterm::queue;

    for region in regions {
        queue!(w, MoveTo(region.x, region.y))?;
        // Build SGR with underline (4) prepended to the original style
        let sgr = if region.sgr.is_empty() {
            "\x1b[4m".to_string()
        } else {
            // Insert underline code after the "\x1b[" prefix
            region.sgr.replacen("\x1b[", "\x1b[4;", 1)
        };
        // SGR style + OSC8 open + text + OSC8 close + SGR reset
        write!(
            w,
            "{sgr}\x1b]8;;{url}\x07{text}\x1b]8;;\x07\x1b[0m",
            sgr = sgr,
            url = region.url,
            text = region.text,
        )?;
    }
    w.flush()?;
    Ok(())
}
