use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::core::Hotkey;
use super::core::hotkey_registry::HotkeyLayer;
use super::modes::test_selector::TestSelectorMode;
use super::panel::Mode;

// ── ModeId ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeId {
    TestSelector,
}

// ── Context ─────────────────────────────────────────────────────────────────

pub struct Context {
    pub active_mode: ModeId,
    pub should_quit: bool,
    pub show_help: bool,
    pub test_selector: TestSelectorMode,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    pub fn new() -> Self {
        Self {
            active_mode: ModeId::TestSelector,
            should_quit: false,
            show_help: false,
            test_selector: TestSelectorMode::new(),
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

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self.active_mode {
            ModeId::TestSelector => self.test_selector.render(area, buf),
        }
    }
}
