use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::Label;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Panel;
use crate::dbg::tui::traits::BlockRenderable;

pub struct BarPanel;

impl BarPanel {
    pub const FOCUS: Hotkey = Hotkey::new('2', "bar");
}

impl BlockRenderable for BarPanel {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
}

impl Panel for BarPanel {
    fn label(&self) -> Label {
        Label::hotkey("bar", Self::FOCUS)
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Bar", vec![])
    }

    fn dispatch(&mut self, _hotkey: &Hotkey) {}
}
