use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::envelope::Envelope;

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
                Span::styled(envelope.from_display(), value_style),
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
