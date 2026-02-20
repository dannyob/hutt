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
    pub title: &'a str,
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
                    // Special entries always visible
                    f.starts_with("+ ")
                        || f.to_lowercase().contains(&filter_lower)
                        // Smart folders: also match the name without @ prefix
                        || f.strip_prefix('@')
                            .is_some_and(|name| name.to_lowercase().contains(&filter_lower))
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
            .title(format!(" {} ", self.title))
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

            // Determine display text and style
            let (display, base_style) = if folder.starts_with("+ ") {
                // Special creation entries — green
                (
                    folder.to_string(),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                )
            } else if let Some(name) = folder.strip_prefix('@') {
                // Smart folder — show with star prefix, cyan/italic
                (
                    format!("\u{2605} {}", name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::ITALIC),
                )
            } else {
                (folder.to_string(), Style::default().fg(Color::White))
            };

            let style = if is_selected {
                base_style
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            // Fill background for selected item
            if is_selected {
                buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
            }

            // Truncate folder name to fit
            let max_w = inner.width as usize;
            let display = truncate_str(&display, max_w);
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

        // Hint at bottom: 'd' to delete
        if inner.height > 3 {
            let hint_y = popup.y + popup.height - 1;
            let hint = " C-d:delete ";
            let hint_x = popup.x + popup.width.saturating_sub(hint.len() as u16 + 1);
            buf.set_string(
                hint_x,
                hint_y,
                hint,
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Smart folder creation popup
// ---------------------------------------------------------------------------

pub struct SmartFolderPopup<'a> {
    pub query: &'a str,
    pub name: &'a str,
    pub phase: u8,
    pub preview: &'a [String],
    pub count: Option<u32>,
}

impl<'a> Widget for SmartFolderPopup<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup_width: u16 = 50;
        let popup_height: u16 = 14;
        let popup = centered_rect(popup_width, popup_height, area);

        Clear.render(popup, buf);

        let title = if self.phase == 0 {
            " New Smart Folder — Query "
        } else {
            " New Smart Folder — Name "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title)
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

        let text_style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::DarkGray);
        let cursor_style = Style::default().fg(Color::White).bg(Color::Gray);

        let mut y = inner.y;

        // Query field
        buf.set_string(inner.x, y, "Query: ", label_style);
        let query_display = truncate_str(self.query, (inner.width as usize).saturating_sub(8));
        buf.set_string(inner.x + 7, y, &query_display, text_style);
        if self.phase == 0 {
            let cx = inner.x + 7 + self.query.len().min(inner.width as usize - 8) as u16;
            if cx < inner.x + inner.width {
                buf.set_string(cx, y, " ", cursor_style);
            }
        }
        y += 1;

        // Name field (only visible in phase 1)
        if self.phase == 1 {
            buf.set_string(inner.x, y, "Name:  ", label_style);
            let name_display = truncate_str(self.name, (inner.width as usize).saturating_sub(8));
            buf.set_string(inner.x + 7, y, &name_display, text_style);
            let cx = inner.x + 7 + self.name.len().min(inner.width as usize - 8) as u16;
            if cx < inner.x + inner.width {
                buf.set_string(cx, y, " ", cursor_style);
            }
        }
        y += 1;

        // Separator
        let sep: String = "\u{2500}".repeat(inner.width as usize);
        buf.set_string(inner.x, y, &sep, Style::default().fg(Color::DarkGray));
        y += 1;

        // Preview results
        if let Some(count) = self.count {
            let count_text = format!("{} result{} found", count, if count == 1 { "" } else { "s" });
            buf.set_string(
                inner.x,
                y,
                &count_text,
                Style::default().fg(Color::Yellow),
            );
            y += 1;

            for subject in self.preview.iter().take(5) {
                if y >= inner.y + inner.height {
                    break;
                }
                let display = truncate_str(subject, inner.width as usize);
                buf.set_string(inner.x + 1, y, &display, Style::default().fg(Color::DarkGray));
                y += 1;
            }
        } else if !self.query.is_empty() {
            buf.set_string(
                inner.x,
                y,
                "Type at least 3 chars to preview...",
                Style::default().fg(Color::DarkGray),
            );
        }

        // Hint at bottom
        let hint = if self.phase == 0 {
            "Enter:confirm query  Esc:cancel"
        } else {
            "Enter:save  Esc:back to query"
        };
        let hint_y = popup.y + popup.height - 1;
        let hint_x = popup.x + 1;
        buf.set_string(hint_x, hint_y, hint, Style::default().fg(Color::DarkGray));
    }
}

// ---------------------------------------------------------------------------
// Maildir creation popup
// ---------------------------------------------------------------------------

pub struct MaildirCreatePopup<'a> {
    pub input: &'a str,
}

impl<'a> Widget for MaildirCreatePopup<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup_width: u16 = 45;
        let popup_height: u16 = 6;
        let popup = centered_rect(popup_width, popup_height, area);

        Clear.render(popup, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" New Maildir Folder ")
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

        let text_style = Style::default().fg(Color::White);
        let label_style = Style::default().fg(Color::DarkGray);
        let cursor_style = Style::default().fg(Color::White).bg(Color::Gray);

        buf.set_string(inner.x, inner.y, "Path: ", label_style);
        let display = truncate_str(self.input, (inner.width as usize).saturating_sub(7));
        buf.set_string(inner.x + 6, inner.y, &display, text_style);
        let cx = inner.x + 6 + self.input.len().min(inner.width as usize - 7) as u16;
        if cx < inner.x + inner.width {
            buf.set_string(cx, inner.y, " ", cursor_style);
        }

        buf.set_string(
            inner.x,
            inner.y + 1,
            "e.g. /Projects/Hutt",
            Style::default().fg(Color::DarkGray),
        );

        // Hint at bottom
        let hint = "Enter:create  Esc:cancel";
        let hint_y = popup.y + popup.height - 1;
        buf.set_string(popup.x + 1, hint_y, hint, Style::default().fg(Color::DarkGray));
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
