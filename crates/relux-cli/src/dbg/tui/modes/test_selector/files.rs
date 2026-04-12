use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::Label;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Panel;
use crate::dbg::tui::traits::BlockRenderable;

pub struct FilesPanel;

impl FilesPanel {
    pub const FOCUS: Hotkey = Hotkey::new('f', "files");
}

impl BlockRenderable for FilesPanel {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
}

impl Panel for FilesPanel {
    fn label(&self) -> Label {
        Label::hotkey("relux files", Self::FOCUS)
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Files", vec![])
    }

    fn dispatch(&mut self, _hotkey: &Hotkey) {}
}
