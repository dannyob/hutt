use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use std::collections::HashSet;

use crate::envelope::{Conversation, Envelope};

pub struct EnvelopeList<'a> {
    pub envelopes: &'a [Envelope],
    pub selected: usize,
    pub offset: usize,
    pub multi_selected: &'a HashSet<u32>,
}

impl<'a> EnvelopeList<'a> {
    /// Calculate the visible range for scrolling.
    pub fn visible_range(
        selected: usize,
        offset: usize,
        height: usize,
        total: usize,
    ) -> (usize, usize) {
        let mut off = offset;
        if selected < off {
            off = selected;
        }
        if selected >= off + height {
            off = selected - height + 1;
        }
        let end = (off + height).min(total);
        (off, end)
    }
}

impl<'a> Widget for EnvelopeList<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.envelopes.is_empty() {
            let style = Style::default().fg(Color::DarkGray);
            buf.set_string(
                area.x + 2,
                area.y + area.height / 2,
                "No messages",
                style,
            );
            return;
        }

        let height = area.height as usize;
        let (start, end) =
            Self::visible_range(self.selected, self.offset, height, self.envelopes.len());

        for (i, envelope) in self.envelopes[start..end].iter().enumerate() {
            let y = area.y + i as u16;
            let idx = start + i;
            let is_selected = idx == self.selected;
            let is_multi = self.multi_selected.contains(&envelope.docid);
            let is_unread = envelope.is_unread();
            let is_flagged = envelope.is_flagged();

            let base_style = if is_selected {
                Style::default().bg(Color::Indexed(236)).fg(Color::White)
            } else {
                Style::default()
            };

            // Fill the line with background
            buf.set_style(Rect::new(area.x, y, area.width, 1), base_style);

            let w = area.width as usize;

            // Multi-select / unread / flag indicator (2 chars)
            let indicator = if is_multi {
                "x "
            } else if is_flagged {
                "* "
            } else if is_unread {
                "> "
            } else {
                "  "
            };
            let ind_style = if is_multi {
                base_style.fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if is_flagged {
                base_style.fg(Color::Yellow)
            } else if is_unread {
                base_style.fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                base_style.fg(Color::DarkGray)
            };
            buf.set_string(area.x, y, indicator, ind_style);

            // From field (up to 20 chars)
            let from = envelope.from_display();
            let from_width = 20.min(w.saturating_sub(2));
            let from_truncated = truncate_str(&from, from_width);
            let from_style = if is_unread {
                base_style.add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            buf.set_string(area.x + 2, y, &from_truncated, from_style);

            // Date (right-aligned, ~10 chars)
            let date = envelope.date_display();
            let date_width = date.len();
            let date_x = if w > date_width + 1 {
                area.x + area.width - date_width as u16 - 1
            } else {
                area.x + area.width - 1
            };
            let date_style = base_style.fg(Color::DarkGray);
            buf.set_string(date_x, y, &date, date_style);

            // Subject (fills the middle)
            let subject_start = area.x + 2 + from_width as u16 + 1;
            let subject_end = date_x.saturating_sub(1);
            if subject_start < subject_end {
                let subject_width = (subject_end - subject_start) as usize;
                let subject = truncate_str(&envelope.subject, subject_width);
                let subj_style = if is_unread {
                    base_style
                } else {
                    base_style.fg(Color::Gray)
                };
                buf.set_string(subject_start, y, &subject, subj_style);
            }
        }
    }
}

pub struct ConversationList<'a> {
    pub conversations: &'a [Conversation],
    pub selected: usize,
    pub offset: usize,
    pub multi_selected: &'a HashSet<u32>,
}

impl<'a> Widget for ConversationList<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.conversations.is_empty() {
            let style = Style::default().fg(Color::DarkGray);
            buf.set_string(
                area.x + 2,
                area.y + area.height / 2,
                "No conversations",
                style,
            );
            return;
        }

        let height = area.height as usize;
        let (start, end) = EnvelopeList::visible_range(
            self.selected,
            self.offset,
            height,
            self.conversations.len(),
        );

        for (i, convo) in self.conversations[start..end].iter().enumerate() {
            let y = area.y + i as u16;
            let idx = start + i;
            let is_selected = idx == self.selected;
            let is_unread = convo.has_unread();
            let is_flagged = convo.has_flagged();
            // Check if any docid in this conversation is multi-selected
            let is_multi = convo
                .all_docids()
                .iter()
                .any(|d| self.multi_selected.contains(d));

            let base_style = if is_selected {
                Style::default().bg(Color::Indexed(236)).fg(Color::White)
            } else {
                Style::default()
            };

            // Fill the line with background
            buf.set_style(Rect::new(area.x, y, area.width, 1), base_style);

            let w = area.width as usize;

            // Multi-select / unread / flag indicator (2 chars)
            let indicator = if is_multi {
                "x "
            } else if is_flagged {
                "* "
            } else if is_unread {
                "> "
            } else {
                "  "
            };
            let ind_style = if is_multi {
                base_style.fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if is_flagged {
                base_style.fg(Color::Yellow)
            } else if is_unread {
                base_style.fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                base_style.fg(Color::DarkGray)
            };
            buf.set_string(area.x, y, indicator, ind_style);

            // Senders (up to 20 chars)
            let senders = convo.senders();
            let senders_width = 20.min(w.saturating_sub(2));
            let senders_truncated = truncate_str(&senders, senders_width);
            let senders_style = if is_unread {
                base_style.add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            buf.set_string(area.x + 2, y, &senders_truncated, senders_style);

            // Date (right-aligned, ~10 chars)
            let date = convo.date_display();
            let date_width = date.len();
            let date_x = if w > date_width + 1 {
                area.x + area.width - date_width as u16 - 1
            } else {
                area.x + area.width - 1
            };
            let date_style = base_style.fg(Color::DarkGray);
            buf.set_string(date_x, y, &date, date_style);

            // Subject + count badge (fills the middle)
            let subject_start = area.x + 2 + senders_width as u16 + 1;
            let subject_end = date_x.saturating_sub(1);
            if subject_start < subject_end {
                let subject_width = (subject_end - subject_start) as usize;
                let count = convo.message_count();
                let badge = if count > 1 {
                    format!(" ({})", count)
                } else {
                    String::new()
                };
                let subj_text = convo.subject();
                let avail = subject_width.saturating_sub(badge.len());
                let mut display = truncate_str(subj_text, avail);
                display.push_str(&badge);
                let subj_style = if is_unread {
                    base_style
                } else {
                    base_style.fg(Color::Gray)
                };
                buf.set_string(subject_start, y, &display, subj_style);
            }
        }
    }
}

/// Truncate a string to fit within `max_width` characters, adding "..." if needed.
fn truncate_str(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 1 {
        "~".to_string()
    } else {
        let mut result: String = chars[..max_width - 1].iter().collect();
        result.push('~');
        result
    }
}
