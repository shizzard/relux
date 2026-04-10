use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::theme;
use super::util::set_cell;

// ── Action ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    Files,
    Foo,
    Bar,
}

#[derive(Clone, Debug)]
pub enum Action {
    Quit,
    ShowHelp,
    FocusPanel(PanelId),
}

// ── Hotkey ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Hotkey {
    pub key: char,
    pub label: String,
    pub description: String,
    pub action: Action,
}

impl Hotkey {
    pub fn new(
        key: char,
        label: impl Into<String>,
        description: impl Into<String>,
        action: Action,
    ) -> Self {
        debug_assert!(key.is_ascii(), "hotkey must be ASCII");
        Self {
            key,
            label: label.into(),
            description: description.into(),
            action,
        }
    }

    /// Render the hotkey label at position, returning the number of columns consumed.
    /// When `active`, the hotkey character is accented; when inactive, fully dimmed.
    pub fn render_label(&self, x: u16, y: u16, buf: &mut Buffer, active: bool) -> u16 {
        if !active {
            return self.render_inactive(x, y, buf);
        }
        self.render_active(x, y, buf)
    }

    fn render_active(&self, x: u16, y: u16, buf: &mut Buffer) -> u16 {
        let key_lower = self.key.to_ascii_lowercase();
        let accent_pos = self
            .label
            .char_indices()
            .position(|(_, c)| c.to_ascii_lowercase() == key_lower);

        match accent_pos {
            Some(pos) => {
                // Hotkey char found in label — accent it in-place.
                let mut col = x;
                for (i, ch) in self.label.chars().enumerate() {
                    let style = if i == pos {
                        theme::HOTKEY_ACTIVE
                    } else {
                        theme::HOTKEY_LABEL
                    };
                    set_cell(col, y, ch, style, buf);
                    col += 1;
                }
                col - x
            }
            None => {
                // Hotkey char not in label — prefix it.
                set_cell(x, y, self.key, theme::HOTKEY_ACTIVE, buf);
                let mut col = x + 1;
                for ch in self.label.chars() {
                    set_cell(col, y, ch, theme::HOTKEY_LABEL, buf);
                    col += 1;
                }
                col - x
            }
        }
    }

    fn render_inactive(&self, x: u16, y: u16, buf: &mut Buffer) -> u16 {
        // When inactive: render label only (no prefix if key not in label), all dimmed.
        let mut col = x;
        for ch in self.label.chars() {
            set_cell(col, y, ch, theme::HOTKEY_INACTIVE, buf);
            col += 1;
        }
        col - x
    }
}

// ── Layer ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// Hotkey resolution falls through to the parent layer.
    Transparent,
    /// Hotkey resolution stops here — parent layers are unreachable.
    Opaque,
}

#[derive(Clone, Debug)]
pub struct HotkeyLayer {
    pub name: String,
    pub kind: LayerKind,
    pub hotkeys: Vec<Hotkey>,
}

impl HotkeyLayer {
    pub fn transparent(name: impl Into<String>, hotkeys: Vec<Hotkey>) -> Self {
        Self {
            name: name.into(),
            kind: LayerKind::Transparent,
            hotkeys,
        }
    }

    pub fn opaque(name: impl Into<String>, hotkeys: Vec<Hotkey>) -> Self {
        Self {
            name: name.into(),
            kind: LayerKind::Opaque,
            hotkeys,
        }
    }
}

// ── Registry ────────────────────────────────────────────────────────────────

pub struct HotkeyRegistry {
    layers: Vec<HotkeyLayer>,
}

impl Default for HotkeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyRegistry {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn push_layer(&mut self, mut layer: HotkeyLayer) {
        // Ensure '?' is always present in every layer.
        if !layer.hotkeys.iter().any(|h| h.key == '?') {
            layer.hotkeys.push(Hotkey::new(
                '?',
                "help",
                "Show hotkey help",
                Action::ShowHelp,
            ));
        }
        self.layers.push(layer);
    }

    pub fn pop_layer(&mut self) -> Option<HotkeyLayer> {
        self.layers.pop()
    }

    /// Dispatch a key event: resolve to a hotkey and return its action.
    /// Walks the layer stack top-down, stops at opaque boundaries.
    pub fn dispatch(&self, event: &KeyEvent) -> Option<Action> {
        let KeyCode::Char(ch) = event.code else {
            return None;
        };
        if event.modifiers != KeyModifiers::NONE && event.modifiers != KeyModifiers::SHIFT {
            return None;
        }
        let ch_lower = ch.to_ascii_lowercase();
        for layer in self.layers.iter().rev() {
            if let Some(hotkey) = layer
                .hotkeys
                .iter()
                .find(|h| h.key.to_ascii_lowercase() == ch_lower)
            {
                return Some(hotkey.action.clone());
            }
            if layer.kind == LayerKind::Opaque {
                return None;
            }
        }
        None
    }

    /// All hotkeys reachable from the current stack top (for status bar rendering).
    pub fn active_hotkeys(&self) -> Vec<&Hotkey> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for layer in self.layers.iter().rev() {
            for hotkey in &layer.hotkeys {
                let key_lower = hotkey.key.to_ascii_lowercase();
                if seen.insert(key_lower) {
                    result.push(hotkey);
                }
            }
            if layer.kind == LayerKind::Opaque {
                break;
            }
        }
        result
    }

    /// Whether a specific hotkey is reachable from the stack top.
    pub fn is_active(&self, key: char) -> bool {
        let key_lower = key.to_ascii_lowercase();
        for layer in self.layers.iter().rev() {
            if layer
                .hotkeys
                .iter()
                .any(|h| h.key.to_ascii_lowercase() == key_lower)
            {
                return true;
            }
            if layer.kind == LayerKind::Opaque {
                return false;
            }
        }
        false
    }

    /// All layers (for help overlay).
    pub fn all_layers(&self) -> &[HotkeyLayer] {
        &self.layers
    }

    /// Render hotkeys into a status bar area (single row).
    pub fn render_status_bar(&self, area: Rect, buf: &mut Buffer) {
        let hotkeys = self.active_hotkeys();
        if hotkeys.is_empty() {
            return;
        }
        // Fill background
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, area.y)];
            cell.set_style(theme::STATUS_BAR_BG);
        }
        let mut col = area.x + 1;
        let max_x = area.x + area.width.saturating_sub(1);
        for hotkey in &hotkeys {
            if col >= max_x {
                break;
            }
            // Render key badge
            set_cell(col, area.y, hotkey.key, theme::STATUS_BAR_KEY, buf);
            col += 1;
            // Space after key
            set_cell(col, area.y, ' ', theme::STATUS_BAR_LABEL, buf);
            col += 1;
            // Render label
            for ch in hotkey.label.chars() {
                if col >= max_x {
                    break;
                }
                set_cell(col, area.y, ch, theme::STATUS_BAR_LABEL, buf);
                col += 1;
            }
            // Separator
            col += 2;
        }
    }
}
