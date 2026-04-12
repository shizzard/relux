use std::path::PathBuf;

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::core::Hotkey;
use super::core::hotkey_registry::HotkeyLayer;
use super::modes::test_selector::TestSelectorMode;
use super::traits::Mode;

// ── ModeId ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeId {
    TestSelector,
}

// ── Context ─────────────────────────────────────────────────────────────────

pub struct Context {
    active_mode: ModeId,
    pub should_quit: bool,
    pub show_help: bool,
    test_selector: TestSelectorMode,
}

impl Context {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            active_mode: ModeId::TestSelector,
            should_quit: false,
            show_help: false,
            test_selector: TestSelectorMode::new(project_root),
        }
    }

    pub fn dispatch_focus(&mut self, hotkey: &Hotkey) {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.dispatch_focus(hotkey),
        }
    }

    pub fn dispatch_panel(&mut self, hotkey: &Hotkey) {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.dispatch_panel(hotkey),
        }
    }

    pub fn panel_hotkeys(&self) -> HotkeyLayer {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.panel_hotkeys(),
        }
    }

    pub fn forward_key_event(&mut self, event: &KeyEvent) {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.forward_key_event(event),
        }
    }

    pub fn mode_hotkeys(&self) -> HotkeyLayer {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.mode_hotkeys(),
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.render(area, buf),
        }
    }
}
