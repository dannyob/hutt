use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::keymap::InputMode;

pub struct TopBar<'a> {
    pub folder: &'a str,
    pub unread_count: usize,
    pub total_count: usize,
    pub mode: &'a InputMode,
    pub thread_subject: Option<&'a str>,
}

impl<'a> Widget for TopBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, style);

        let left = match self.mode {
            InputMode::ThreadView => {
                let subj = self.thread_subject.unwrap_or("Thread");
                format!(" {} ", subj)
            }
            _ => {
                if self.folder.starts_with('@') {
                    format!(" \u{2605} {} ", &self.folder[1..])
                } else {
                    format!(" {} ", self.folder)
                }
            }
        };

        let right = if self.unread_count > 0 {
            format!(" {}/{} unread ", self.unread_count, self.total_count)
        } else {
            format!(" {} messages ", self.total_count)
        };

        // Render left-aligned label
        let left_spans = Line::from(vec![Span::styled(
            &left,
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]);
        buf.set_line(area.x, area.y, &left_spans, area.width);

        // Render right-aligned count
        let right_len = right.len() as u16;
        if area.width > right_len + left.len() as u16 {
            let rx = area.x + area.width - right_len;
            buf.set_string(rx, area.y, &right, style);
        }
    }
}

pub struct BottomBar<'a> {
    pub mode: &'a InputMode,
    pub pending_key: Option<String>,
    pub search_input: Option<&'a str>,
    pub status_message: Option<&'a str>,
    pub filter_desc: Option<&'a str>,
    pub selection_count: usize,
}

impl<'a> BottomBar<'a> {
    fn hints_for_mode(&self) -> &'static str {
        match self.mode {
            InputMode::Normal => {
                "j/k:nav e:archive #:trash s:star /:search Enter:thread ?:help"
            }
            InputMode::Search => "Type to search | Enter:submit Esc:cancel",
            InputMode::ThreadView => {
                "j/k:nav o:expand e:archive r:reply q:back ?:help"
            }
            InputMode::FolderPicker => {
                "j/k:nav Enter:select C-d:delete Esc:cancel | type to filter"
            }
            InputMode::CommandPalette => "j/k:nav Enter:select Esc:cancel | type to filter",
            InputMode::Help => "j/k:scroll ?/q/Esc:close",
            InputMode::SmartFolderCreate => "Type query | Enter:confirm Esc:cancel",
            InputMode::SmartFolderName => "Type name | Enter:save Esc:back",
            InputMode::MaildirCreate => "Type path | Enter:create Esc:cancel",
            InputMode::MoveToFolder => "Enter:move Esc:cancel | type to filter",
        }
    }
}

impl<'a> Widget for BottomBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, style);

        // Priority: search input > status message > normal hints
        if let Some(search) = self.search_input {
            let search_style = Style::default()
                .bg(Color::DarkGray)
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            let prompt = " /";
            buf.set_string(area.x, area.y, prompt, search_style);
            buf.set_string(area.x + prompt.len() as u16, area.y, search, style);
            // Cursor indicator
            let cursor_x = area.x + prompt.len() as u16 + search.len() as u16;
            if cursor_x < area.x + area.width {
                buf.set_string(
                    cursor_x,
                    area.y,
                    "_",
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::SLOW_BLINK),
                );
            }
            return;
        }

        let mut text = String::new();

        if let Some(status) = self.status_message {
            text.push_str(&format!(" {} | ", status));
        }

        if self.selection_count > 0 {
            text.push_str(&format!(" [{} selected] ", self.selection_count));
        }

        if let Some(filter) = self.filter_desc {
            text.push_str(&format!(" [{}] ", filter));
        }

        if let Some(ref pending) = self.pending_key {
            text.push_str(&format!(" {}... | ", pending));
        }

        text.push_str(&format!(" {}", self.hints_for_mode()));

        buf.set_string(area.x, area.y, &text, style);
    }
}
