pub mod bordered;
pub mod context;
pub mod core;
pub mod help_overlay;
pub mod modes;
pub mod panel;
pub mod scrollable;
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

use context::Context;
use core::Hotkey;
use core::hotkey_registry::HotkeyLayer;
use core::hotkey_registry::HotkeyRegistry;
use help_overlay::HelpOverlay;
use panel::Mode;

// ── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    ctx: Context,
    registry: HotkeyRegistry,
}

impl App {
    pub fn new() -> Self {
        let ctx = Context::new();

        let global = HotkeyLayer::new(
            "Global",
            vec![Hotkey::new('q', "quit"), Hotkey::new('?', "help")],
        );
        let mut registry = HotkeyRegistry::new(global);
        registry.set_mode(ctx.test_selector.mode_hotkeys());
        registry.set_panel(ctx.test_selector.panel_hotkeys());

        Self { ctx, registry }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.ctx.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_event()?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        if area.height < 5 {
            return;
        }

        self.ctx.render(area, frame.buffer_mut());

        if self.ctx.show_help {
            let overlay = HelpOverlay::new(&self.registry);
            overlay.render(area, frame.buffer_mut());
        }
    }

    fn handle_event(&mut self) -> io::Result<()> {
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if self.ctx.show_help {
                self.ctx.show_help = false;
                return Ok(());
            }
            self.registry.handle_key(&key, &mut self.ctx);
        }
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
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
