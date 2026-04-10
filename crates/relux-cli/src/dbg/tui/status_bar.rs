use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::hotkey::HotkeyRegistry;
use super::traits::Panel;

/// Bottom-row status bar displaying active hotkey hints.
pub struct StatusBar<'a> {
    registry: &'a HotkeyRegistry,
}

impl<'a> StatusBar<'a> {
    pub fn new(registry: &'a HotkeyRegistry) -> Self {
        Self { registry }
    }
}

impl Panel for StatusBar<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        // Render on the first (and only expected) row of the area.
        let bar_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        self.registry.render_status_bar(bar_area, buf);
    }
}
