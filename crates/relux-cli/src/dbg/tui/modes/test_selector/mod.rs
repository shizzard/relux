mod bar;
mod files;
mod foo;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use bar::BarPanel;
use files::FilesPanel;
use foo::FooPanel;

use crate::dbg::tui::bordered::Bordered;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Mode;
use crate::dbg::tui::panel::Panel;

// ── PanelId ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    Files,
    Foo,
    Bar,
}

// ── TestSelectorMode ────────────────────────────────────────────────────────

pub struct TestSelectorMode {
    pub focused: PanelId,
    files: FilesPanel,
    foo: FooPanel,
    bar: BarPanel,
}

impl Default for TestSelectorMode {
    fn default() -> Self {
        Self::new()
    }
}

impl TestSelectorMode {
    pub fn new() -> Self {
        Self {
            focused: PanelId::Files,
            files: FilesPanel,
            foo: FooPanel,
            bar: BarPanel,
        }
    }
}

impl Mode for TestSelectorMode {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 {
            return;
        }

        let top_height = area.height / 2;
        let bottom_height = area.height - top_height;
        let top_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: top_height,
        };
        let bottom_left_width = area.width / 2;
        let bottom_right_width = area.width - bottom_left_width;
        let bottom_left = Rect {
            x: area.x,
            y: area.y + top_height,
            width: bottom_left_width,
            height: bottom_height,
        };
        let bottom_right = Rect {
            x: area.x + bottom_left_width,
            y: area.y + top_height,
            width: bottom_right_width,
            height: bottom_height,
        };

        Bordered::new(&self.files)
            .top_item(Box::new(self.files.label()))
            .focused(self.focused == PanelId::Files)
            .render(top_area, buf);

        Bordered::new(&self.foo)
            .top_item(Box::new(self.foo.label()))
            .focused(self.focused == PanelId::Foo)
            .render(bottom_left, buf);

        Bordered::new(&self.bar)
            .top_item(Box::new(self.bar.label()))
            .focused(self.focused == PanelId::Bar)
            .render(bottom_right, buf);
    }

    fn dispatch_focus(&mut self, hotkey: &Hotkey) {
        if *hotkey == FilesPanel::FOCUS {
            self.focused = PanelId::Files;
        } else if *hotkey == FooPanel::FOCUS {
            self.focused = PanelId::Foo;
        } else if *hotkey == BarPanel::FOCUS {
            self.focused = PanelId::Bar;
        }
    }

    fn dispatch_panel(&mut self, hotkey: &Hotkey) {
        match self.focused {
            PanelId::Files => self.files.dispatch(hotkey),
            PanelId::Foo => self.foo.dispatch(hotkey),
            PanelId::Bar => self.bar.dispatch(hotkey),
        }
    }

    fn mode_hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new(
            "Test Selector",
            vec![FilesPanel::FOCUS, FooPanel::FOCUS, BarPanel::FOCUS],
        )
    }

    fn panel_hotkeys(&self) -> HotkeyLayer {
        match self.focused {
            PanelId::Files => self.files.hotkeys(),
            PanelId::Foo => self.foo.hotkeys(),
            PanelId::Bar => self.bar.hotkeys(),
        }
    }
}
