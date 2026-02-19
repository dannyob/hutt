use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use crate::keymap::Action;

use super::folder_picker::centered_rect;

#[derive(Clone)]
pub struct PaletteEntry {
    pub name: String,
    pub description: String,
    pub shortcut: Option<String>,
    pub action: Action,
}

impl PaletteEntry {
    /// Return all available actions with their descriptions and keyboard shortcuts.
    pub fn all_actions() -> Vec<PaletteEntry> {
        vec![
            // Navigation
            PaletteEntry {
                name: "Move Down".into(),
                description: "Move to the next message".into(),
                shortcut: Some("j / Down".into()),
                action: Action::MoveDown,
            },
            PaletteEntry {
                name: "Move Up".into(),
                description: "Move to the previous message".into(),
                shortcut: Some("k / Up".into()),
                action: Action::MoveUp,
            },
            PaletteEntry {
                name: "Jump to Top".into(),
                description: "Go to the first message".into(),
                shortcut: Some("gg".into()),
                action: Action::JumpTop,
            },
            PaletteEntry {
                name: "Jump to Bottom".into(),
                description: "Go to the last message".into(),
                shortcut: Some("G".into()),
                action: Action::JumpBottom,
            },
            PaletteEntry {
                name: "Scroll Preview Down".into(),
                description: "Scroll the preview pane down".into(),
                shortcut: Some("Space".into()),
                action: Action::ScrollPreviewDown,
            },
            PaletteEntry {
                name: "Scroll Preview Up".into(),
                description: "Scroll the preview pane up".into(),
                shortcut: Some("Shift+Space".into()),
                action: Action::ScrollPreviewUp,
            },
            PaletteEntry {
                name: "Half Page Down".into(),
                description: "Move half a page down".into(),
                shortcut: Some("Ctrl+d".into()),
                action: Action::HalfPageDown,
            },
            PaletteEntry {
                name: "Half Page Up".into(),
                description: "Move half a page up".into(),
                shortcut: Some("Ctrl+u".into()),
                action: Action::HalfPageUp,
            },
            // Triage
            PaletteEntry {
                name: "Archive".into(),
                description: "Archive the selected message".into(),
                shortcut: Some("e".into()),
                action: Action::Archive,
            },
            PaletteEntry {
                name: "Trash".into(),
                description: "Move message to trash".into(),
                shortcut: Some("#".into()),
                action: Action::Trash,
            },
            PaletteEntry {
                name: "Spam".into(),
                description: "Mark message as spam".into(),
                shortcut: Some("!".into()),
                action: Action::Spam,
            },
            PaletteEntry {
                name: "Toggle Read".into(),
                description: "Toggle read/unread status".into(),
                shortcut: Some("u".into()),
                action: Action::ToggleRead,
            },
            PaletteEntry {
                name: "Toggle Star".into(),
                description: "Toggle starred/flagged status".into(),
                shortcut: Some("s".into()),
                action: Action::ToggleStar,
            },
            PaletteEntry {
                name: "Undo".into(),
                description: "Undo the last action".into(),
                shortcut: Some("z".into()),
                action: Action::Undo,
            },
            // Folder switching
            PaletteEntry {
                name: "Go to Inbox".into(),
                description: "Switch to Inbox folder".into(),
                shortcut: Some("gi".into()),
                action: Action::GoInbox,
            },
            PaletteEntry {
                name: "Go to Archive".into(),
                description: "Switch to Archive folder".into(),
                shortcut: Some("ga".into()),
                action: Action::GoArchive,
            },
            PaletteEntry {
                name: "Go to Drafts".into(),
                description: "Switch to Drafts folder".into(),
                shortcut: Some("gd".into()),
                action: Action::GoDrafts,
            },
            PaletteEntry {
                name: "Go to Sent".into(),
                description: "Switch to Sent folder".into(),
                shortcut: Some("gt".into()),
                action: Action::GoSent,
            },
            PaletteEntry {
                name: "Go to Trash".into(),
                description: "Switch to Trash folder".into(),
                shortcut: Some("g#".into()),
                action: Action::GoTrash,
            },
            PaletteEntry {
                name: "Go to Spam".into(),
                description: "Switch to Spam folder".into(),
                shortcut: Some("g!".into()),
                action: Action::GoSpam,
            },
            PaletteEntry {
                name: "Switch Folder".into(),
                description: "Open folder picker".into(),
                shortcut: Some("gl".into()),
                action: Action::GoFolderPicker,
            },
            // Search & Filters
            PaletteEntry {
                name: "Search".into(),
                description: "Search messages".into(),
                shortcut: Some("/".into()),
                action: Action::EnterSearch,
            },
            PaletteEntry {
                name: "Filter Unread".into(),
                description: "Show only unread messages".into(),
                shortcut: Some("U".into()),
                action: Action::FilterUnread,
            },
            PaletteEntry {
                name: "Filter Starred".into(),
                description: "Show only starred messages".into(),
                shortcut: Some("S".into()),
                action: Action::FilterStarred,
            },
            PaletteEntry {
                name: "Filter Needs Reply".into(),
                description: "Show messages needing a reply".into(),
                shortcut: Some("R".into()),
                action: Action::FilterNeedsReply,
            },
            // Multi-select
            PaletteEntry {
                name: "Toggle Select".into(),
                description: "Toggle selection on current message".into(),
                shortcut: Some("x".into()),
                action: Action::ToggleSelect,
            },
            PaletteEntry {
                name: "Select Down".into(),
                description: "Select current message and move down".into(),
                shortcut: Some("J".into()),
                action: Action::SelectDown,
            },
            PaletteEntry {
                name: "Select Up".into(),
                description: "Select current message and move up".into(),
                shortcut: Some("K".into()),
                action: Action::SelectUp,
            },
            // Thread view
            PaletteEntry {
                name: "Open Thread".into(),
                description: "Open the selected thread".into(),
                shortcut: Some("Enter".into()),
                action: Action::OpenThread,
            },
            // Compose
            PaletteEntry {
                name: "Compose".into(),
                description: "Compose a new message".into(),
                shortcut: Some("c".into()),
                action: Action::Compose,
            },
            PaletteEntry {
                name: "Reply".into(),
                description: "Reply to the selected message".into(),
                shortcut: Some("r".into()),
                action: Action::Reply,
            },
            PaletteEntry {
                name: "Reply All".into(),
                description: "Reply to all recipients".into(),
                shortcut: Some("a".into()),
                action: Action::ReplyAll,
            },
            PaletteEntry {
                name: "Forward".into(),
                description: "Forward the selected message".into(),
                shortcut: Some("f".into()),
                action: Action::Forward,
            },
            // Linkability
            PaletteEntry {
                name: "Copy Message URL".into(),
                description: "Copy message URL to clipboard".into(),
                shortcut: Some("y".into()),
                action: Action::CopyMessageUrl,
            },
            PaletteEntry {
                name: "Copy Thread URL".into(),
                description: "Copy thread URL to clipboard".into(),
                shortcut: Some("Y".into()),
                action: Action::CopyThreadUrl,
            },
            PaletteEntry {
                name: "Open in Browser".into(),
                description: "Open message in browser".into(),
                shortcut: Some("Ctrl+o".into()),
                action: Action::OpenInBrowser,
            },
            // Sync
            PaletteEntry {
                name: "Sync Mail".into(),
                description: "Sync mail from server".into(),
                shortcut: Some("Ctrl+r".into()),
                action: Action::SyncMail,
            },
            // System
            PaletteEntry {
                name: "Quit".into(),
                description: "Quit hutt".into(),
                shortcut: Some("q".into()),
                action: Action::Quit,
            },
        ]
    }

    /// Check if this entry matches the given filter string.
    /// Uses case-insensitive substring matching on name and description.
    fn matches(&self, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let filter_lower = filter.to_lowercase();
        let haystack = format!("{} {}", self.name, self.description).to_lowercase();

        // Simple fuzzy: all characters of the filter must appear in order
        let mut haystack_chars = haystack.chars();
        for fc in filter_lower.chars() {
            loop {
                match haystack_chars.next() {
                    Some(hc) if hc == fc => break,
                    Some(_) => continue,
                    None => return false,
                }
            }
        }
        true
    }
}

pub struct CommandPalette<'a> {
    pub entries: &'a [PaletteEntry],
    pub filter: &'a str,
    pub selected: usize,
}

impl<'a> CommandPalette<'a> {
    /// Return the filtered list of entries matching the current filter.
    pub fn filtered_entries(&self) -> Vec<&'a PaletteEntry> {
        self.entries
            .iter()
            .filter(|e| e.matches(self.filter))
            .collect()
    }
}

impl<'a> Widget for CommandPalette<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let filtered = self.filtered_entries();

        // Popup dimensions: 60 chars wide, min(entries.len()*2 + 4, 20) tall
        // Each entry takes 2 lines (name + description), plus border and filter
        let popup_width: u16 = 60;
        let popup_height: u16 = ((filtered.len() * 2 + 4) as u16).min(20).max(6);

        let popup = centered_rect(popup_width, popup_height, area);

        // Clear the area behind the popup
        Clear.render(popup, buf);

        // Draw border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" Command Palette ")
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

        // Search input line with cursor
        let filter_style = Style::default().fg(Color::White);
        let cursor_style = Style::default().fg(Color::White).bg(Color::Gray);
        let prompt = "> ";
        buf.set_string(inner.x, inner.y, prompt, filter_style);
        buf.set_string(inner.x + 2, inner.y, self.filter, filter_style);
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

        // Entry list (each entry takes 2 lines: name+shortcut, then description)
        let list_start_y = inner.y + 2;
        let list_height = inner.height.saturating_sub(2) as usize;

        let sel = self.selected.min(filtered.len().saturating_sub(1));

        // Calculate scroll offset: each entry is 2 lines
        let scroll_offset = if sel * 2 >= list_height {
            (sel * 2 - list_height + 2) / 2
        } else {
            0
        };

        let mut y = list_start_y;
        for (i, entry) in filtered.iter().skip(scroll_offset).enumerate() {
            let display_idx = scroll_offset + i;
            let is_selected = display_idx == sel;

            if y >= inner.y + inner.height {
                break;
            }

            // Line 1: name (bold) + shortcut (right-aligned, dark gray)
            let name_style = if is_selected {
                Style::default()
                    .bg(Color::Indexed(236))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            };

            let base_bg = if is_selected {
                Style::default().bg(Color::Indexed(236))
            } else {
                Style::default()
            };

            // Fill background for selected entry (both lines)
            if is_selected {
                buf.set_style(Rect::new(inner.x, y, inner.width, 1), base_bg);
                if y + 1 < inner.y + inner.height {
                    buf.set_style(Rect::new(inner.x, y + 1, inner.width, 1), base_bg);
                }
            }

            // Name
            let name_display = truncate_str(&entry.name, inner.width as usize);
            buf.set_string(inner.x + 1, y, &name_display, name_style);

            // Shortcut (right-aligned)
            if let Some(ref shortcut) = entry.shortcut {
                let shortcut_style = if is_selected {
                    Style::default()
                        .bg(Color::Indexed(236))
                        .fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let sc_len = shortcut.len() as u16;
                let sc_x = (inner.x + inner.width).saturating_sub(sc_len + 1);
                if sc_x > inner.x + 1 + entry.name.len() as u16 {
                    buf.set_string(sc_x, y, shortcut, shortcut_style);
                }
            }

            y += 1;
            if y >= inner.y + inner.height {
                break;
            }

            // Line 2: description (gray)
            let desc_style = if is_selected {
                Style::default()
                    .bg(Color::Indexed(236))
                    .fg(Color::Gray)
            } else {
                Style::default().fg(Color::Gray)
            };
            let desc_display = truncate_str(&entry.description, (inner.width as usize).saturating_sub(2));
            buf.set_string(inner.x + 2, y, &desc_display, desc_style);

            y += 1;
        }

        // If no matches, show hint
        if filtered.is_empty() && list_start_y < inner.y + inner.height {
            buf.set_string(
                inner.x + 1,
                list_start_y,
                "No matching commands",
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
