pub mod bordered;
pub mod help_overlay;
pub mod hotkey;
pub mod scrollable;
pub mod status_bar;
pub mod theme;
pub mod traits;
pub mod util;

use std::io;

use crossterm::event::Event;
use crossterm::event::KeyEventKind;
use crossterm::event::{self};
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::enable_raw_mode;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use ratatui::prelude::*;

use bordered::BorderItem;
use bordered::Bordered;
use help_overlay::HelpOverlay;
use hotkey::Action;
use hotkey::Hotkey;
use hotkey::HotkeyLayer;
use hotkey::HotkeyRegistry;
use hotkey::PanelId;
use status_bar::StatusBar;
use traits::Overlay;
use traits::Panel;

// ── Placeholder panel for smoke test ────────────────────────────────────────

struct EmptyPanel;

impl Panel for EmptyPanel {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
}

// ── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    should_quit: bool,
    show_help: bool,
    focused: PanelId,
    registry: HotkeyRegistry,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let mut registry = HotkeyRegistry::new();
        registry.push_layer(HotkeyLayer::transparent(
            "Global",
            vec![
                Hotkey::new('q', "quit", "Quit the debugger", Action::Quit),
                Hotkey::new(
                    'f',
                    "files",
                    "Focus the files panel",
                    Action::FocusPanel(PanelId::Files),
                ),
                Hotkey::new(
                    '1',
                    "foo",
                    "Focus the foo panel",
                    Action::FocusPanel(PanelId::Foo),
                ),
                Hotkey::new(
                    '2',
                    "bar",
                    "Focus the bar panel",
                    Action::FocusPanel(PanelId::Bar),
                ),
            ],
        ));
        Self {
            should_quit: false,
            show_help: false,
            focused: PanelId::Files,
            registry,
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_event()?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        if area.height < 5 {
            return;
        }

        // Split: content area + 1-row status bar at bottom.
        let content_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height - 1,
        };
        let status_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };

        // Split content: top half + bottom half.
        let top_height = content_area.height / 2;
        let bottom_height = content_area.height - top_height;
        let top_area = Rect {
            x: content_area.x,
            y: content_area.y,
            width: content_area.width,
            height: top_height,
        };
        let bottom_left_width = content_area.width / 2;
        let bottom_right_width = content_area.width - bottom_left_width;
        let bottom_left = Rect {
            x: content_area.x,
            y: content_area.y + top_height,
            width: bottom_left_width,
            height: bottom_height,
        };
        let bottom_right = Rect {
            x: content_area.x + bottom_left_width,
            y: content_area.y + top_height,
            width: bottom_right_width,
            height: bottom_height,
        };

        // Panels
        let files = Bordered::new(EmptyPanel)
            .left_item(BorderItem::title_with_hotkey(Hotkey::new(
                'f',
                "relux files",
                "Focus the files panel",
                Action::FocusPanel(PanelId::Files),
            )))
            .focused(self.focused == PanelId::Files);
        files.render(top_area, frame.buffer_mut(), &self.registry);

        let panel_1 = Bordered::new(EmptyPanel)
            .left_item(BorderItem::title_with_hotkey(Hotkey::new(
                '1',
                "foo",
                "Focus the foo panel",
                Action::FocusPanel(PanelId::Foo),
            )))
            .focused(self.focused == PanelId::Foo);
        panel_1.render(bottom_left, frame.buffer_mut(), &self.registry);

        let panel_2 = Bordered::new(EmptyPanel)
            .left_item(BorderItem::title_with_hotkey(Hotkey::new(
                '2',
                "bar",
                "Focus the bar panel",
                Action::FocusPanel(PanelId::Bar),
            )))
            .focused(self.focused == PanelId::Bar);
        panel_2.render(bottom_right, frame.buffer_mut(), &self.registry);

        // Status bar
        let status = StatusBar::new(&self.registry);
        status.render(status_area, frame.buffer_mut());

        // Help overlay (on top of everything)
        if self.show_help {
            let overlay = HelpOverlay::new(&self.registry);
            overlay.render(area, frame.buffer_mut());
        }
    }

    fn handle_event(&mut self) -> io::Result<()> {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if self.show_help {
                self.show_help = false;
                return Ok(());
            }
            if let Some(action) = self.registry.dispatch(&key) {
                self.apply(action);
            }
        }
        Ok(())
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::ShowHelp => self.show_help = true,
            Action::FocusPanel(id) => self.focused = id,
        }
    }
}

// ── Terminal setup ──────────────────────────────────────────────────────────

pub fn init_terminal() -> io::Result<DefaultTerminal> {
    enable_raw_mode()?;
    execute!(io::stderr(), EnterAlternateScreen)?;
    Ok(ratatui::init())
}

pub fn restore_terminal() {
    ratatui::restore();
}
