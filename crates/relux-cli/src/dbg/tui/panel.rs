use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::core::Hotkey;
use super::core::Label;
use super::core::hotkey_registry::HotkeyLayer;
use super::traits::BlockRenderable;

// ── Panel ───────────────────────────────────────────────────────────────────

pub trait Panel<S>: BlockRenderable {
    /// Label for the border. Includes hotkey if focusable.
    fn label(&self) -> Label;

    /// Hotkeys active when this panel is focused. Layer name = panel name.
    fn hotkeys(&self) -> HotkeyLayer;

    /// Handle a resolved hotkey. Only called for hotkeys from this panel's layer.
    fn dispatch(&mut self, hotkey: &Hotkey, mode_state: &mut S);

    /// Handle a raw key event (arrows, Enter, etc.).
    /// Returns `true` if mode state was mutated.
    fn handle_key_event(&mut self, _event: &KeyEvent, _mode_state: &mut S) -> bool {
        false
    }

    /// Called on sibling panels when another panel mutated the mode state.
    fn mode_state_changed(&mut self, _mode_state: &S) {}
}

// ── Mode ────────────────────────────────────────────────────────────────────

pub trait Mode {
    /// Render the mode's layout and panels.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Handle a mode-layer hotkey (panel focus switch).
    fn dispatch_focus(&mut self, hotkey: &Hotkey);

    /// Handle a panel-layer hotkey (forwarded to focused panel).
    fn dispatch_panel(&mut self, hotkey: &Hotkey);

    /// Forward a raw key event to the focused panel.
    fn forward_key_event(&mut self, event: &KeyEvent);

    /// Mode-level hotkeys (panel focus keys). Set once on mode enter.
    fn mode_hotkeys(&self) -> HotkeyLayer;

    /// Focused panel's hotkeys. Updated on focus change.
    fn panel_hotkeys(&self) -> HotkeyLayer;
}
