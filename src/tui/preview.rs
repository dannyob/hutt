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

        // Add body lines from RenderedMessage
        if let Some(body) = self.body {
            for rich_line in &body.lines {
                let spans: Vec<Span> = rich_line
                    .iter()
                    .map(|s| Span::styled(s.text.clone(), span_style(&s.kind)))
                    .collect();
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
        SpanKind::Link(_) => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
        SpanKind::Emphasis => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::ITALIC),
        SpanKind::Strong => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        SpanKind::Code => Style::default().fg(Color::Green),
    }
}
