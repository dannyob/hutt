use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use super::folder_picker::centered_rect;

struct HelpSection {
    title: &'static str,
    keys: &'static [(&'static str, &'static str)],
}

const SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        keys: &[
            ("j / Down", "Move down"),
            ("k / Up", "Move up"),
            ("gg", "Jump to top"),
            ("G", "Jump to bottom"),
            ("Space", "Scroll preview down"),
            ("Shift+Space", "Scroll preview up"),
            ("Ctrl+d", "Half page down"),
            ("Ctrl+u", "Half page up"),
        ],
    },
    HelpSection {
        title: "Triage",
        keys: &[
            ("e", "Archive"),
            ("#", "Trash"),
            ("!", "Spam"),
            ("u", "Toggle read/unread"),
            ("s", "Toggle star"),
            ("z", "Undo"),
        ],
    },
    HelpSection {
        title: "Folders",
        keys: &[
            ("gi", "Go to Inbox"),
            ("ga", "Go to Archive"),
            ("gd", "Go to Drafts"),
            ("gt", "Go to Sent"),
            ("g#", "Go to Trash"),
            ("g!", "Go to Spam"),
            ("gl", "Folder picker"),
        ],
    },
    HelpSection {
        title: "Search & Filters",
        keys: &[
            ("/", "Search"),
            ("U", "Filter unread"),
            ("S", "Filter starred"),
            ("R", "Filter needs reply"),
        ],
    },
    HelpSection {
        title: "Selection",
        keys: &[
            ("x", "Toggle select"),
            ("J", "Select + move down"),
            ("K", "Select + move up"),
        ],
    },
    HelpSection {
        title: "Thread",
        keys: &[
            ("Enter", "Open thread"),
            ("o", "Toggle expand"),
            ("O", "Expand/collapse all"),
            ("q / Esc", "Close thread"),
        ],
    },
    HelpSection {
        title: "Compose",
        keys: &[
            ("c", "Compose new"),
            ("r", "Reply"),
            ("a", "Reply all"),
            ("f", "Forward"),
        ],
    },
    HelpSection {
        title: "Links & Clipboard",
        keys: &[
            ("y", "Copy message URL"),
            ("Y", "Copy thread URL"),
            ("Ctrl+o", "Open in browser"),
        ],
    },
    HelpSection {
        title: "Other",
        keys: &[
            ("Ctrl+k", "Command palette"),
            ("Ctrl+r", "Sync mail"),
            ("?", "This help"),
            ("q", "Quit"),
        ],
    },
];

pub struct HelpOverlay {
    pub scroll: u16,
}

impl Widget for HelpOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup_width: u16 = 56;
        let popup_height: u16 = area.height.min(30).max(10);
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

        // Build all lines first, then apply scroll
        let mut lines: Vec<(Style, String)> = Vec::new();
        let key_col_width = 14;

        for (si, section) in SECTIONS.iter().enumerate() {
            if si > 0 {
                lines.push((Style::default(), String::new()));
            }
            lines.push((
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                format!(" {}", section.title),
            ));

            for (key, desc) in section.keys {
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
                // Section header, footer, or blank line — render as-is
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
