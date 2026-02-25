use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};

use crate::keymap::InputMode;
use crate::tui::{TabRegion, TabRegionKind};

pub struct TopBar<'a> {
    pub folder: &'a str,
    pub unread_count: usize,
    pub total_count: usize,
    pub mode: &'a InputMode,
    pub thread_subject: Option<&'a str>,
    pub account_name: Option<&'a str>,
    pub conversations_mode: bool,
    pub tabs: &'a [String],
    pub tab_scroll: usize,
    pub multi_account: bool,
}

/// Result of rendering the tab bar — the hit regions for mouse clicks.
pub struct TabBarRegions {
    pub regions: Vec<TabRegion>,
}

impl<'a> TopBar<'a> {
    /// Render the tab bar and return hit regions for mouse interaction.
    pub fn render_with_regions(self, area: Rect, buf: &mut Buffer) -> TabBarRegions {
        let bar_style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, bar_style);

        let mut regions: Vec<TabRegion> = Vec::new();

        // In thread view, show the thread subject (old behavior)
        if *self.mode == InputMode::ThreadView {
            let subj = self.thread_subject.unwrap_or("Thread");
            let text = format!(" {} ", subj);
            buf.set_string(
                area.x,
                area.y,
                &text,
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            return TabBarRegions { regions };
        }

        let mut x = area.x;

        // ── Account badge ──────────────────────────────────────────
        if self.multi_account {
            let name = self.account_name.unwrap_or("?");
            let badge = format!(" {} ", name);
            let badge_len = badge.len() as u16;
            let account_style = Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            buf.set_string(x, area.y, &badge, account_style);
            regions.push(TabRegion {
                x_start: x,
                x_end: x + badge_len,
                kind: TabRegionKind::Account,
            });
            x += badge_len;
            // Separator
            buf.set_string(x, area.y, " ", bar_style);
            x += 1;
        }

        // ── Right-aligned counts ───────────────────────────────────
        let unit = if self.conversations_mode { "threads" } else { "messages" };
        let right = if self.unread_count > 0 {
            format!(" {}/{} unread ", self.unread_count, self.total_count)
        } else {
            format!(" {} {} ", self.total_count, unit)
        };
        let right_len = right.len() as u16;
        let right_x = area.x + area.width - right_len;
        // We'll render the right count later, but reserve the space now
        let overflow_width = 3u16; // " … "
        let available_for_tabs = if right_x > x + overflow_width {
            right_x - x - overflow_width
        } else {
            0
        };

        // ── Tabs ───────────────────────────────────────────────────
        if self.tabs.is_empty() {
            // Fallback: just show current folder
            let text = format!(" {} ", self.folder);
            let style = Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD);
            buf.set_string(x, area.y, &text, style);
        } else {
            // Pinned inbox (tab 0) is always shown
            let inbox = &self.tabs[0];
            let inbox_label = tab_label(inbox);
            let inbox_len = inbox_label.len() as u16;
            let inbox_selected = self.folder == inbox;
            let inbox_style = if inbox_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                tab_style(inbox)
            };
            buf.set_string(x, area.y, &inbox_label, inbox_style);
            regions.push(TabRegion {
                x_start: x,
                x_end: x + inbox_len,
                kind: TabRegionKind::Tab(0),
            });
            x += inbox_len;

            // Scrollable tabs (starting from tab_scroll, skipping index 0)
            let scroll_start = self.tab_scroll.max(1);
            let mut all_fit = true;
            for i in scroll_start..self.tabs.len() {
                let label = tab_label(&self.tabs[i]);
                let label_len = label.len() as u16;
                if x + label_len > area.x + available_for_tabs + (area.x + area.width - right_x - overflow_width) {
                    // Won't fit — we need the overflow indicator
                    all_fit = false;
                    break;
                }
                let selected = self.folder == self.tabs[i];
                let style = if selected {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    tab_style(&self.tabs[i])
                };
                buf.set_string(x, area.y, &label, style);
                regions.push(TabRegion {
                    x_start: x,
                    x_end: x + label_len,
                    kind: TabRegionKind::Tab(i),
                });
                x += label_len;
            }

            // Overflow "…"
            if !all_fit || scroll_start > 1 {
                let overflow_label = " \u{2026} ";
                let overflow_style = Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::Gray);
                // Position just before the right count
                let overflow_x = right_x - overflow_width;
                if overflow_x >= x {
                    buf.set_string(overflow_x, area.y, overflow_label, overflow_style);
                    regions.push(TabRegion {
                        x_start: overflow_x,
                        x_end: overflow_x + overflow_width,
                        kind: TabRegionKind::Overflow,
                    });
                }
            }
        }

        // ── Right count ────────────────────────────────────────────
        buf.set_string(right_x, area.y, &right, bar_style);

        TabBarRegions { regions }
    }
}

fn tab_label(folder: &str) -> String {
    format!(" {} ", folder)
}

fn tab_style(folder: &str) -> Style {
    let fg = if folder.starts_with('#') {
        Color::Cyan
    } else if folder.starts_with('@') {
        Color::Yellow
    } else {
        Color::White
    };
    Style::default().bg(Color::DarkGray).fg(fg)
}

pub struct BottomBar<'a> {
    pub mode: &'a InputMode,
    pub pending_key: Option<String>,
    pub status_message: Option<&'a str>,
    pub filter_desc: Option<&'a str>,
    pub selection_count: usize,
    pub conversations_mode: bool,
    pub sort_label: Option<&'a str>,
}

impl<'a> BottomBar<'a> {
    fn hints_for_mode(&self) -> &'static str {
        match self.mode {
            InputMode::Normal if self.conversations_mode => {
                "j/k:nav e:archive #:trash s:star /:search V:messages ?:help"
            }
            InputMode::Normal => {
                "j/k:nav e:archive #:trash s:star /:search V:conversations ?:help"
            }
            InputMode::Search => "Type to search | ↑↓:history Enter:submit Esc:cancel",
            InputMode::ThreadView => {
                "j/k:nav o:expand e:archive r:reply q:back ?:help"
            }
            InputMode::FolderPicker => {
                "j/k:nav Enter:select C-e:edit C-d:delete Esc:cancel | filter"
            }
            InputMode::CommandPalette => "j/k:nav Enter:select Esc:cancel | type to filter",
            InputMode::Help => "j/k:scroll ?/q/Esc:close",
            InputMode::SmartFolderCreate => "Type query | Enter:confirm Esc:cancel",
            InputMode::SmartFolderName => "Type name | Enter:save Esc:back",
            InputMode::MaildirCreate => "Type path | Enter:create Esc:cancel",
            InputMode::MoveToFolder => "Enter:move Esc:cancel | type to filter",
            InputMode::AccountPicker => "j/k:nav Enter:select Esc:cancel",
            InputMode::SortPicker => "(d)ate (f)rom (s)ubject (t)o | Esc:cancel",
            InputMode::AttachmentPopup => "j/k:nav Enter:select Esc:cancel",
        }
    }
}

impl<'a> Widget for BottomBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        buf.set_style(area, style);

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

        if let Some(sort) = self.sort_label {
            text.push_str(&format!(" [{}] ", sort));
        }

        if let Some(ref pending) = self.pending_key {
            text.push_str(&format!(" {}... | ", pending));
        }

        text.push_str(&format!(" {}", self.hints_for_mode()));

        buf.set_string(area.x, area.y, &text, style);
    }
}
