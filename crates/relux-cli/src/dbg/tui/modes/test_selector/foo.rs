use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::Label;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Panel;
use crate::dbg::tui::traits::BlockRenderable;

pub struct FooPanel;

impl FooPanel {
    pub const FOCUS: Hotkey = Hotkey::new('1', "foo");
}

impl BlockRenderable for FooPanel {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
}

impl Panel for FooPanel {
    fn label(&self) -> Label {
        Label::hotkey("foo", Self::FOCUS)
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Foo", vec![])
    }

    fn dispatch(&mut self, _hotkey: &Hotkey) {}
}
