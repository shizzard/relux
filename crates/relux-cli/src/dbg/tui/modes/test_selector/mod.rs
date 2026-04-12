mod details;
mod files;

use std::path::PathBuf;

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use details::DetailsPanel;
use files::FilesPanel;

use crate::dbg::tui::bordered::Bordered;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Mode;
use crate::dbg::tui::panel::Panel;

// ── Mode state ─────────────────────────────────────────────────────────────

pub struct TestSelectorState {
    pub selected_file: PathBuf,
}

// ── TestSelectorMode ───────────────────────────────────────────────────────

pub struct TestSelectorMode {
    mode_state: TestSelectorState,
    files: FilesPanel,
    details: DetailsPanel,
}

impl TestSelectorMode {
    pub fn new(project_root: PathBuf) -> Self {
        let tests_dir = relux_core::config::tests_dir(&project_root);
        let files = FilesPanel::new(tests_dir.clone());
        let mut details = DetailsPanel::new(tests_dir);
        let mode_state = TestSelectorState {
            selected_file: files.selected_file_path(),
        };
        details.mode_state_changed(&mode_state);
        Self {
            mode_state,
            files,
            details,
        }
    }
}

impl Mode for TestSelectorMode {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 || area.width < 4 {
            return;
        }

        let files_height = area.height * 60 / 100;
        let details_height = area.height - files_height;
        let files_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: files_height,
        };
        let details_area = Rect {
            x: area.x,
            y: area.y + files_height,
            width: area.width,
            height: details_height,
        };

        Bordered::new(&self.files)
            .top_item(Box::new(self.files.label()))
            .focused(true)
            .render(files_area, buf);

        Bordered::new(&self.details)
            .top_item(Box::new(self.details.label()))
            .focused(false)
            .render(details_area, buf);
    }

    fn dispatch_focus(&mut self, _hotkey: &Hotkey) {
        // Only one focusable panel — nothing to switch.
    }

    fn dispatch_panel(&mut self, hotkey: &Hotkey) {
        self.files.dispatch(hotkey, &mut self.mode_state);
        self.details.mode_state_changed(&self.mode_state);
    }

    fn forward_key_event(&mut self, event: &KeyEvent) {
        let changed = self.files.handle_key_event(event, &mut self.mode_state);
        if changed {
            self.details.mode_state_changed(&self.mode_state);
        }
    }

    fn mode_hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Test Selector", vec![])
    }

    fn panel_hotkeys(&self) -> HotkeyLayer {
        self.files.hotkeys()
    }
}
