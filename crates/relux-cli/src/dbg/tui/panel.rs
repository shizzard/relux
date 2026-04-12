use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::core::Hotkey;
use super::core::Label;
use super::core::hotkey_registry::HotkeyLayer;
use super::traits::BlockRenderable;

// ── Panel ───────────────────────────────────────────────────────────────────

pub trait Panel: BlockRenderable {
    /// Label for the border. Includes hotkey if focusable.
    fn label(&self) -> Label;

    /// Hotkeys active when this panel is focused. Layer name = panel name.
    fn hotkeys(&self) -> HotkeyLayer;

    /// Handle a resolved hotkey. Only called for hotkeys from this panel's layer.
    fn dispatch(&mut self, hotkey: &Hotkey);
}

// ── Mode ────────────────────────────────────────────────────────────────────

pub trait Mode {
    /// Render the mode's layout and panels.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Handle a mode-layer hotkey (panel focus switch).
    fn dispatch_focus(&mut self, hotkey: &Hotkey);

    /// Handle a panel-layer hotkey (forwarded to focused panel).
    fn dispatch_panel(&mut self, hotkey: &Hotkey);

    /// Mode-level hotkeys (panel focus keys). Set once on mode enter.
    fn mode_hotkeys(&self) -> HotkeyLayer;

    /// Focused panel's hotkeys. Updated on focus change.
    fn panel_hotkeys(&self) -> HotkeyLayer;
}
