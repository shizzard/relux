use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use super::Hotkey;
use crate::dbg::tui::context::Context;

// ── Slot ────────────────────────────────────────────────────────────────────

enum Slot {
    Global,
    Mode,
    Panel,
}

// ── Layer ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyLayer {
    /// A named set of hotkeys.
    Set { name: String, hotkeys: Vec<Hotkey> },
    /// Forward all key events to the panel, bypassing hotkey resolution.
    CatchAll,
}

impl HotkeyLayer {
    pub fn new(name: impl Into<String>, hotkeys: Vec<Hotkey>) -> Self {
        Self::Set {
            name: name.into(),
            hotkeys,
        }
    }

    pub fn empty() -> Self {
        Self::Set {
            name: String::new(),
            hotkeys: Vec::new(),
        }
    }

    pub fn hotkeys(&self) -> &[Hotkey] {
        match self {
            Self::Set { hotkeys, .. } => hotkeys,
            Self::CatchAll => &[],
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Set { name, .. } => name,
            Self::CatchAll => "",
        }
    }
}

// ── Registry ────────────────────────────────────────────────────────────────

pub struct HotkeyRegistry {
    global: HotkeyLayer,
    mode: HotkeyLayer,
    panel: HotkeyLayer,
}

impl HotkeyRegistry {
    pub fn new(global: HotkeyLayer) -> Self {
        Self {
            global,
            mode: HotkeyLayer::empty(),
            panel: HotkeyLayer::empty(),
        }
    }

    pub fn set_mode(&mut self, layer: HotkeyLayer) {
        self.mode = layer;
    }

    pub fn set_panel(&mut self, layer: HotkeyLayer) {
        self.panel = layer;
    }

    /// Resolve a key char to a hotkey, walking panel → mode → global.
    fn resolve(&self, key: char) -> Option<(Slot, &Hotkey)> {
        let key_lower = key.to_ascii_lowercase();
        for hotkey in self.panel.hotkeys() {
            if hotkey.key.to_ascii_lowercase() == key_lower {
                return Some((Slot::Panel, hotkey));
            }
        }
        for hotkey in self.mode.hotkeys() {
            if hotkey.key.to_ascii_lowercase() == key_lower {
                return Some((Slot::Mode, hotkey));
            }
        }
        for hotkey in self.global.hotkeys() {
            if hotkey.key.to_ascii_lowercase() == key_lower {
                return Some((Slot::Global, hotkey));
            }
        }
        None
    }

    /// Handle a key event: resolve, dispatch to the appropriate handler.
    pub fn handle_key(&mut self, event: &KeyEvent, ctx: &mut Context) {
        // When the panel is in CatchAll mode, forward everything to it.
        if matches!(self.panel, HotkeyLayer::CatchAll) {
            ctx.forward_key_event(event);
            // The panel may have exited CatchAll — re-read its hotkeys.
            self.panel = ctx.panel_hotkeys();
            return;
        }

        let KeyCode::Char(ch) = event.code else {
            ctx.forward_key_event(event);
            return;
        };
        if event.modifiers != KeyModifiers::NONE && event.modifiers != KeyModifiers::SHIFT {
            return;
        }

        let Some((slot, hotkey)) = self.resolve(ch) else {
            return;
        };
        let hotkey = hotkey.clone();

        match slot {
            Slot::Global => {
                if hotkey.key == 'q' {
                    ctx.should_quit = true;
                } else if hotkey.key == '?' {
                    ctx.show_help = true;
                }
            }
            Slot::Mode => {
                ctx.dispatch_focus(&hotkey);
                let new_panel = ctx.panel_hotkeys();
                if new_panel != self.panel {
                    self.panel = new_panel;
                }
            }
            Slot::Panel => {
                ctx.dispatch_panel(&hotkey);
                // The panel may have entered CatchAll — re-read its hotkeys.
                self.panel = ctx.panel_hotkeys();
            }
        }
    }

    /// All hotkeys reachable, deduplicating by key (panel > mode > global).
    pub fn active_hotkeys(&self) -> Vec<&Hotkey> {
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for layer in [&self.panel, &self.mode, &self.global] {
            for hotkey in layer.hotkeys() {
                let key_lower = hotkey.key.to_ascii_lowercase();
                if seen.insert(key_lower) {
                    result.push(hotkey);
                }
            }
        }
        result
    }

    /// All layers for help overlay display.
    pub fn all_layers(&self) -> [&HotkeyLayer; 3] {
        [&self.global, &self.mode, &self.panel]
    }
}
