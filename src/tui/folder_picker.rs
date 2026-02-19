use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

pub struct FolderPicker<'a> {
    pub folders: &'a [String],
    pub selected: usize,
    pub filter: &'a str,
}

/// Compute a centered rectangle of the given width and height within `area`.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(x, y, w, h)
}

impl<'a> FolderPicker<'a> {
    /// Return the list of folders matching the current filter (case-insensitive substring).
    pub fn filtered_folders(&self) -> Vec<(usize, &'a String)> {
        let filter_lower = self.filter.to_lowercase();
        self.folders
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                if filter_lower.is_empty() {
                    true
                } else {
                    f.to_lowercase().contains(&filter_lower)
                }
            })
            .collect()
    }
}

impl<'a> Widget for FolderPicker<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let filtered = self.filtered_folders();

        // Popup dimensions: 40 chars wide, min(filtered.len()+4, 20) tall
        // +4 accounts for: top border, filter line, bottom border, and a bit of padding
        let popup_width: u16 = 40;
        let popup_height: u16 = ((filtered.len() + 4) as u16).min(20);

        let popup = centered_rect(popup_width, popup_height, area);

        // Clear the area behind the popup
        Clear.render(popup, buf);

        // Draw border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(" Switch Folder ")
            .title_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        block.render(popup, buf);

        // Inner area (inside the border)
        let inner = Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Filter input line with cursor
        let filter_style = Style::default().fg(Color::White);
        let cursor_style = Style::default()
            .fg(Color::White)
            .bg(Color::Gray);
        let prompt = "> ";
        buf.set_string(inner.x, inner.y, prompt, filter_style);
        buf.set_string(inner.x + 2, inner.y, self.filter, filter_style);
        // Cursor block after the filter text
        let cursor_x = inner.x + 2 + self.filter.len() as u16;
        if cursor_x < inner.x + inner.width {
            buf.set_string(cursor_x, inner.y, " ", cursor_style);
        }

        // Separator line
        if inner.height > 1 {
            let sep: String = "\u{2500}".repeat(inner.width as usize);
            buf.set_string(
                inner.x,
                inner.y + 1,
                &sep,
                Style::default().fg(Color::DarkGray),
            );
        }

        // Folder list
        let list_start_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(2) as usize;

        // Clamp selected index to filtered range
        let sel = self.selected.min(filtered.len().saturating_sub(1));

        // Calculate scroll offset so selected item is visible
        let scroll_offset = if sel >= list_height {
            sel - list_height + 1
        } else {
            0
        };

        for (i, (_orig_idx, folder)) in filtered
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
        {
            let y = list_start_y + i as u16;
            if y >= inner.y + inner.height {
                break;
            }

            let display_idx = scroll_offset + i;
            let is_selected = display_idx == sel;

            let style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            // Fill background for selected item
            if is_selected {
                buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            }

            // Truncate folder name to fit
            let max_w = inner.width as usize;
            let display = truncate_str(folder, max_w);
            buf.set_string(inner.x + 1, y, &display, style);
        }

        // If no matches, show hint
        if filtered.is_empty() && list_start_y < inner.y + inner.height {
            buf.set_string(
                inner.x + 1,
                list_start_y,
                "No matching folders",
                Style::default().fg(Color::DarkGray),
            );
        }
    }
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
