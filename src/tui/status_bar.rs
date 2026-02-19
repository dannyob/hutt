use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

pub struct TopBar<'a> {
    pub folder: &'a str,
    pub unread_count: usize,
    pub total_count: usize,
}

impl<'a> Widget for TopBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, style);

        let left = format!(" {} ", self.folder);
        let right = if self.unread_count > 0 {
            format!(" {}/{} unread ", self.unread_count, self.total_count)
        } else {
            format!(" {} messages ", self.total_count)
        };

        // Render left-aligned folder name
        let left_spans = Line::from(vec![
            Span::styled(
                &left,
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let x = area.x;
        buf.set_line(x, area.y, &left_spans, area.width);

        // Render right-aligned count
        let right_len = right.len() as u16;
        if area.width > right_len + left.len() as u16 {
            let rx = area.x + area.width - right_len;
            buf.set_string(rx, area.y, &right, style);
        }
    }
}

pub struct BottomBar<'a> {
    pub hints: &'a str,
    pub pending_key: Option<&'a str>,
}

impl<'a> Widget for BottomBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, style);

        let text = if let Some(pending) = self.pending_key {
            format!(" {}… | {}", pending, self.hints)
        } else {
            format!(" {}", self.hints)
        };

        buf.set_string(area.x, area.y, &text, style);
    }
}
