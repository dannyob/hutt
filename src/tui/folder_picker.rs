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
    pub title: &'a str,
}

impl<'a> SmartFolderPopup<'a> {
    /// Compute the popup layout and return the area where the textarea should be rendered.
    pub fn textarea_area(&self, area: Rect) -> Rect {
        let label_len: u16 = 7;
        let max_width = (area.width * 80 / 100).max(40);
        let content_len = self.query.len().max(self.name.len()) as u16 + label_len + 2;
        let popup_width = content_len.clamp(60, max_width);
        let inner_width = popup_width.saturating_sub(2);
        let query_field_width = inner_width.saturating_sub(label_len) as usize;
        let query_lines = wrap_text(self.query, query_field_width);
        let query_line_count = query_lines.len().max(1) as u16;
        let preview_lines = if self.count.is_some() {
            1 + self.preview.len().min(5) as u16
        } else if !self.query.is_empty() {
            1
        } else {
            0
        };
        let name_line: u16 = if self.phase == 1 { 1 } else { 0 };
        let content_height = query_line_count + name_line + 1 + preview_lines + 1;
        let popup_height = (content_height + 2).clamp(8, area.height.saturating_sub(4));
        let popup = centered_rect(popup_width, popup_height, area);

        // The active field area
        if self.phase == 0 {
            // Query field: starts at popup.y + 1 (border), x + 1 (border) + label_len
            Rect::new(
                popup.x + 1 + label_len,
                popup.y + 1,
                inner_width.saturating_sub(label_len),
                query_line_count,
            )
        } else {
            // Name field: after query lines
            Rect::new(
                popup.x + 1 + label_len,
                popup.y + 1 + query_line_count,
                inner_width.saturating_sub(label_len),
                1,
            )
        }
    }
}

impl<'a> Widget for SmartFolderPopup<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let label_len: u16 = 7; // "Query: " or "Name:  "
        let max_width = (area.width * 80 / 100).max(40);

        // Compute how wide the popup needs to be for the query/name content
        let content_len = self.query.len().max(self.name.len()) as u16 + label_len + 2; // +2 for cursor + padding
        let popup_width = content_len.clamp(60, max_width);

        // Inner content width for wrapping calculations
        let inner_width = popup_width.saturating_sub(2); // borders
        let query_field_width = inner_width.saturating_sub(label_len) as usize;

        // Word-wrap the query to calculate how many lines it needs
        let query_lines = wrap_text(self.query, query_field_width);
        let query_line_count = query_lines.len().max(1) as u16;

        // Height: query lines + name line + separator + preview (up to 6) + hint + borders
        let preview_lines = if self.count.is_some() {
            1 + self.preview.len().min(5) as u16
        } else if !self.query.is_empty() {
            1
        } else {
            0
        };
        let name_line: u16 = if self.phase == 1 { 1 } else { 0 };
        let content_height = query_line_count + name_line + 1 /* sep */ + preview_lines + 1 /* hint */;
        let popup_height = (content_height + 2 /* borders */).clamp(8, area.height.saturating_sub(4));

        let popup = centered_rect(popup_width, popup_height, area);

        Clear.render(popup, buf);

        let title = if self.phase == 0 {
            format!(" {} — Query ", self.title)
        } else {
            format!(" {} — Name ", self.title)
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

        let mut y = inner.y;

        // Query field — label only; textarea renders the text
        buf.set_string(inner.x, y, "Query: ", label_style);
        // If phase != 0 (not active), render the query text ourselves
        if self.phase != 0 {
            for line in &query_lines {
                if y >= inner.y + inner.height {
                    break;
                }
                buf.set_string(inner.x + label_len, y, line, text_style);
                y += 1;
            }
        } else {
            // Skip lines — textarea will render here
            y += query_line_count;
        }

        // Name field
        if self.phase == 1 && y < inner.y + inner.height {
            // Label only; textarea renders the name text
            buf.set_string(inner.x, y, "Name:  ", label_style);
            y += 1;
        } else if self.phase != 1 && y < inner.y + inner.height {
            // Not editing name — show it as static text if we have one
            if !self.name.is_empty() {
                buf.set_string(inner.x, y, "Name:  ", label_style);
                let name_display = truncate_str(self.name, query_field_width);
                buf.set_string(inner.x + label_len, y, &name_display, text_style);
            }
            y += 1;
        }

        // Separator
        if y < inner.y + inner.height {
            let sep: String = "\u{2500}".repeat(inner.width as usize);
            buf.set_string(inner.x, y, &sep, Style::default().fg(Color::DarkGray));
            y += 1;
        }

        // Preview results
        if let Some(count) = self.count {
            if y < inner.y + inner.height {
                let count_text = format!("{} result{} found", count, if count == 1 { "" } else { "s" });
                buf.set_string(
                    inner.x,
                    y,
                    &count_text,
                    Style::default().fg(Color::Yellow),
                );
                y += 1;
            }

            for subject in self.preview.iter().take(5) {
                if y >= inner.y + inner.height {
                    break;
                }
                let display = truncate_str(subject, inner.width as usize);
                buf.set_string(inner.x + 1, y, &display, Style::default().fg(Color::DarkGray));
                y += 1;
            }
        } else if !self.query.is_empty() && y < inner.y + inner.height {
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

/// Word-wrap text to fit within a given width, breaking at spaces.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if text.len() <= width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut remaining = text;

    while remaining.len() > width {
        // Find the last space within the width
        let break_at = remaining[..width]
            .rfind(' ')
            .map(|i| i + 1) // break after the space
            .unwrap_or(width); // hard-break if no space found

        lines.push(remaining[..break_at].trim_end().to_string());
        remaining = &remaining[break_at..];
    }
    if !remaining.is_empty() {
        lines.push(remaining.to_string());
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_short_text() {
        assert_eq!(wrap_text("hello world", 40), vec!["hello world"]);
    }

    #[test]
    fn wrap_at_space() {
        assert_eq!(
            wrap_text("from:a@example.com or from:b@example.com or from:c@example.com", 30),
            vec![
                "from:a@example.com or",
                "from:b@example.com or",
                "from:c@example.com",
            ]
        );
    }

    #[test]
    fn wrap_no_space_hard_break() {
        assert_eq!(
            wrap_text("abcdefghij", 5),
            vec!["abcde", "fghij"]
        );
    }

    #[test]
    fn wrap_empty() {
        assert_eq!(wrap_text("", 40), vec![""]);
    }
}
