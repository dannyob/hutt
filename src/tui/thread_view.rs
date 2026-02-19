use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::envelope::Envelope;

pub struct ThreadMessage {
    pub envelope: Envelope,
    pub body: Option<String>,
    pub expanded: bool,
}

pub struct ThreadView<'a> {
    pub messages: &'a [ThreadMessage],
    pub selected: usize,
    pub scroll: u16,
}

impl<'a> Widget for ThreadView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.messages.is_empty() {
            let style = Style::default().fg(Color::DarkGray);
            buf.set_string(area.x + 2, area.y + area.height / 2, "No messages", style);
            return;
        }

        // Thread header: "[N messages in thread]"
        let header = format!("[{} messages in thread]", self.messages.len());
        let header_style = Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC);

        // Collect all lines to render, then apply scroll
        let mut lines: Vec<RenderedLine> = Vec::new();

        lines.push(RenderedLine {
            content: vec![(header, header_style)],
            msg_index: None,
        });
        lines.push(RenderedLine {
            content: Vec::new(),
            msg_index: None,
        });

        for (idx, msg) in self.messages.iter().enumerate() {
            // Separator between cards (skip before the first one)
            if idx > 0 {
                let sep: String = "\u{2500}".repeat(area.width.saturating_sub(2) as usize);
                let sep_style = Style::default().fg(Color::DarkGray);
                lines.push(RenderedLine {
                    content: vec![(sep, sep_style)],
                    msg_index: None,
                });
            }

            let is_selected = idx == self.selected;

            // Build header line: From | Date | expand indicator
            let from = msg.envelope.from_display();
            let date = msg.envelope.date_display();
            let expand_indicator = if msg.expanded { "[-]" } else { "[+]" };

            let bg = if is_selected {
                Color::Indexed(236)
            } else {
                Color::Reset
            };
            let header_base = Style::default().bg(bg);

            let from_style = header_base
                .fg(Color::White)
                .add_modifier(Modifier::BOLD);
            let date_style = header_base.fg(Color::DarkGray);
            let indicator_style = header_base.fg(Color::Cyan);

            lines.push(RenderedLine {
                content: vec![
                    (format!("{}", from), from_style),
                    (" | ".to_string(), header_base.fg(Color::DarkGray)),
                    (format!("{}", date), date_style),
                    (" ".to_string(), header_base),
                    (expand_indicator.to_string(), indicator_style),
                ],
                msg_index: Some(idx),
            });

            // If expanded, show body
            if msg.expanded {
                if let Some(ref body) = msg.body {
                    for line in body.lines() {
                        let style = if line.starts_with('>') {
                            header_base.fg(Color::DarkGray)
                        } else {
                            header_base.fg(Color::White)
                        };
                        lines.push(RenderedLine {
                            content: vec![(line.to_string(), style)],
                            msg_index: Some(idx),
                        });
                    }
                } else {
                    lines.push(RenderedLine {
                        content: vec![("Loading\u{2026}".to_string(), header_base.fg(Color::DarkGray))],
                        msg_index: Some(idx),
                    });
                }
                // Blank line after body
                lines.push(RenderedLine {
                    content: Vec::new(),
                    msg_index: Some(idx),
                });
            }
        }

        // Render with scroll offset
        let scroll = self.scroll as usize;
        let visible_height = area.height as usize;

        for (row, line) in lines.iter().skip(scroll).take(visible_height).enumerate() {
            let y = area.y + row as u16;

            // If this line belongs to the selected message, fill background
            if let Some(msg_idx) = line.msg_index {
                if msg_idx == self.selected {
                    let bg_style = Style::default().bg(Color::Indexed(236));
                    buf.set_style(Rect::new(area.x, y, area.width, 1), bg_style);
                }
            }

            // Render spans
            let mut x = area.x + 1; // 1 char left padding
            for (text, style) in &line.content {
                let max_chars = (area.x + area.width).saturating_sub(x) as usize;
                let truncated = truncate_str(text, max_chars);
                buf.set_string(x, y, &truncated, *style);
                x += truncated.len() as u16;
            }
        }
    }
}

/// Internal type for pre-computed rendered lines.
struct RenderedLine {
    /// Spans to render: (text, style) pairs
    content: Vec<(String, Style)>,
    /// Which message index this line belongs to (None for thread chrome)
    msg_index: Option<usize>,
}

/// Truncate a string to fit within `max_width` characters, adding "\u{2026}" if needed.
fn truncate_str(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 1 {
        "\u{2026}".to_string()
    } else {
        let mut result: String = chars[..max_width - 1].iter().collect();
        result.push('\u{2026}');
        result
    }
}
