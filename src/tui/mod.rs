pub mod envelope_list;
pub mod preview;
pub mod status_bar;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};
use std::io;
use std::time::Duration;
use tokio::time::Instant;

use crate::envelope::Envelope;
use crate::keymap::{Action, KeyMapper};
use crate::mime_render::{self, RenderCache};
use crate::mu_client::{FindOpts, MuClient};

use self::envelope_list::EnvelopeList;
use self::preview::PreviewPane;
use self::status_bar::{BottomBar, TopBar};

pub struct App {
    pub current_folder: String,
    pub current_query: String,
    pub envelopes: Vec<Envelope>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub preview_scroll: u16,
    pub preview_cache: RenderCache,
    pub mu: MuClient,
    pub keymap: KeyMapper,
    pub should_quit: bool,
    /// Track which message_id is currently loaded in preview
    pub preview_loaded_for: Option<String>,
}

impl App {
    pub async fn new(mu: MuClient) -> Result<Self> {
        Ok(Self {
            current_folder: "/Inbox".to_string(),
            current_query: String::new(),
            envelopes: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            preview_scroll: 0,
            preview_cache: RenderCache::new(),
            mu,
            keymap: KeyMapper::new(),
            should_quit: false,
            preview_loaded_for: None,
        })
    }

    pub async fn load_folder(&mut self) -> Result<()> {
        let query = if self.current_folder.starts_with('/') {
            format!("maildir:{}", self.current_folder)
        } else {
            // Treat as raw mu query
            self.current_folder.clone()
        };
        self.current_query = query.clone();
        self.envelopes = self.mu.find(&query, &FindOpts::default()).await?;
        self.selected = 0;
        self.scroll_offset = 0;
        self.preview_scroll = 0;
        self.preview_loaded_for = None;
        Ok(())
    }

    fn selected_envelope(&self) -> Option<&Envelope> {
        self.envelopes.get(self.selected)
    }

    fn ensure_preview_loaded(&mut self, width: u16) {
        let envelope = match self.envelopes.get(self.selected) {
            Some(e) => e,
            None => return,
        };

        let msg_id = &envelope.message_id;

        // Already cached?
        if self.preview_cache.get(msg_id, width).is_some() {
            return;
        }

        // Render synchronously (file I/O, fast enough for now)
        match mime_render::render_message(&envelope.path, width) {
            Ok(text) => {
                self.preview_cache.insert(msg_id.clone(), width, text);
            }
            Err(e) => {
                self.preview_cache.insert(
                    msg_id.clone(),
                    width,
                    format!("[Error rendering message: {}]", e),
                );
            }
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::MoveDown => {
                if self.selected + 1 < self.envelopes.len() {
                    self.selected += 1;
                    self.preview_scroll = 0;
                }
            }
            Action::MoveUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.preview_scroll = 0;
                }
            }
            Action::JumpTop => {
                self.selected = 0;
                self.preview_scroll = 0;
            }
            Action::JumpBottom => {
                if !self.envelopes.is_empty() {
                    self.selected = self.envelopes.len() - 1;
                    self.preview_scroll = 0;
                }
            }
            Action::ScrollPreviewDown => {
                self.preview_scroll = self.preview_scroll.saturating_add(5);
            }
            Action::ScrollPreviewUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(5);
            }
            Action::Quit => {
                self.should_quit = true;
            }
            Action::Noop => {}
        }
    }
}

pub async fn run(mut app: App) -> Result<()> {
    // Load initial folder
    app.load_folder().await?;

    // Setup terminal
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let sequence_timeout = Duration::from_millis(1000);
    let mut last_key_time = Instant::now();

    loop {
        // Render
        let preview_width = {
            let size = terminal.size()?;
            // 65% of total width for preview, minus some padding
            (size.width * 65 / 100).saturating_sub(4)
        };
        app.ensure_preview_loaded(preview_width);

        terminal.draw(|frame| {
            let size = frame.area();

            // Top bar / content / bottom bar
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // top bar
                    Constraint::Min(3),    // content
                    Constraint::Length(1), // bottom bar
                ])
                .split(size);

            // Top bar
            let unread = app.envelopes.iter().filter(|e| e.is_unread()).count();
            let top = TopBar {
                folder: &app.current_folder,
                unread_count: unread,
                total_count: app.envelopes.len(),
            };
            frame.render_widget(top, outer[0]);

            // Content: envelope list | preview
            let content = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(35),
                    Constraint::Percentage(65),
                ])
                .split(outer[1]);

            // Envelope list
            let env_list = EnvelopeList {
                envelopes: &app.envelopes,
                selected: app.selected,
                offset: app.scroll_offset,
            };
            frame.render_widget(env_list, content[0]);

            // Update scroll offset after rendering knows the height
            let height = content[0].height as usize;
            let (new_offset, _) = EnvelopeList::visible_range(
                app.selected,
                app.scroll_offset,
                height,
                app.envelopes.len(),
            );
            app.scroll_offset = new_offset;

            // Preview pane
            let envelope = app.selected_envelope();
            let body = envelope
                .and_then(|e| app.preview_cache.get(&e.message_id, preview_width));
            let preview = PreviewPane {
                envelope,
                body,
                scroll: app.preview_scroll,
            };
            frame.render_widget(preview, content[1]);

            // Bottom bar
            let pending = if app.keymap.has_pending() {
                Some("g")
            } else {
                None
            };
            let bottom = BottomBar {
                hints: "j/k:nav  gg:top  G:bottom  Space:scroll  q:quit",
                pending_key: pending,
            };
            frame.render_widget(bottom, outer[2]);
        })?;

        if app.should_quit {
            break;
        }

        // Handle key sequence timeout
        if app.keymap.has_pending() && last_key_time.elapsed() > sequence_timeout {
            app.keymap.cancel_pending();
        }

        // Poll for events with timeout
        let timeout = if app.keymap.has_pending() {
            sequence_timeout
        } else {
            Duration::from_millis(100)
        };

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // crossterm sends Press and Release on some platforms
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                last_key_time = Instant::now();
                let action = app.keymap.handle(key);
                app.handle_action(action);
            }
        }
    }

    // Cleanup
    terminal::disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    app.mu.quit().await?;

    Ok(())
}
