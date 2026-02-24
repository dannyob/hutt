use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use super::folder_picker::centered_rect;

pub struct HelpOverlay {
    pub scroll: u16,
    /// (section_title, [(key_string, description)])
    pub sections: Vec<(String, Vec<(String, String)>)>,
    /// Custom bindings not in standard sections: (key_string, description)
    pub extras: Vec<(String, String)>,
}

impl Widget for HelpOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup_width: u16 = 56;
        let popup_height: u16 = area.height.clamp(10, 30);
        let popup = centered_rect(popup_width, popup_height, area);

        Clear.render(popup, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Keyboard Shortcuts ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        block.render(popup, buf);

        let inner = Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Build all lines: (style, text)
        let mut lines: Vec<(Style, String)> = Vec::new();
        let key_col_width = 16;

        let header_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        for (si, (title, items)) in self.sections.iter().enumerate() {
            if si > 0 {
                lines.push((Style::default(), String::new()));
            }
            lines.push((header_style, format!(" {}", title)));

            for (key, desc) in items {
                let padded_key = format!("  {:width$}", key, width = key_col_width);
                lines.push((Style::default(), format!("{}{}", padded_key, desc)));
            }
        }

        // Custom bindings section
        if !self.extras.is_empty() {
            lines.push((Style::default(), String::new()));
            lines.push((header_style, " Custom Bindings".to_string()));
            for (key, desc) in &self.extras {
                let padded_key = format!("  {:width$}", key, width = key_col_width);
                lines.push((Style::default(), format!("{}{}", padded_key, desc)));
            }
        }

        // Footer
        lines.push((Style::default(), String::new()));
        lines.push((
            Style::default().fg(Color::DarkGray),
            " j/k:scroll  ?/q/Esc:close".to_string(),
        ));

        let scroll = self.scroll as usize;
        let max_scroll = lines.len().saturating_sub(inner.height as usize);
        let scroll = scroll.min(max_scroll);

        let key_style = Style::default().fg(Color::Cyan);
        let desc_style = Style::default().fg(Color::White);

        for (i, (style, line)) in lines.iter().skip(scroll).enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let y = inner.y + i as u16;

            if style.fg == Some(Color::Yellow) || style.fg == Some(Color::DarkGray) || line.is_empty()
            {
                // Section header, footer, or blank line
                buf.set_string(inner.x, y, line, *style);
            } else {
                // Key-description line: color the key part in cyan
                let key_end = (key_col_width + 2).min(line.len());
                let key_part = &line[..key_end];
                let desc_part = &line[key_end..];
                buf.set_string(inner.x, y, key_part, key_style);
                buf.set_string(inner.x + key_end as u16, y, desc_part, desc_style);
            }
        }
    }
}
