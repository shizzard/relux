use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::bordered::Bordered;
use super::core::HotkeyRegistry;
use super::theme;
use super::traits::BlockRenderable;
use super::util::set_cell;

// ── Help content panel ──────────────────────────────────────────────────────

/// The inner content of the help overlay: hotkey list grouped by layer.
struct HelpContent<'a> {
    registry: &'a HotkeyRegistry,
}

impl BlockRenderable for HelpContent<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let layers = self.registry.all_layers();
        let mut row = area.y;
        let max_row = area.y + area.height;

        for (layer_idx, layer) in layers.iter().enumerate() {
            if row >= max_row {
                break;
            }
            // Layer name header
            for (i, ch) in layer.name.chars().enumerate() {
                if area.x + i as u16 >= area.x + area.width {
                    break;
                }
                set_cell(area.x + i as u16, row, ch, theme::HELP_LAYER_NAME, buf);
            }
            row += 1;

            // Hotkeys in this layer
            for hotkey in &layer.hotkeys {
                if row >= max_row {
                    break;
                }
                let mut col = area.x + 2;
                set_cell(col, row, hotkey.key, theme::HELP_KEY, buf);
                col += 3;
                for ch in hotkey.description.chars() {
                    if col >= area.x + area.width {
                        break;
                    }
                    set_cell(col, row, ch, theme::HELP_DESCRIPTION, buf);
                    col += 1;
                }
                row += 1;
            }

            // Blank separator between layers (except last)
            if layer_idx + 1 < layers.len() {
                row += 1;
            }
        }
    }
}

// ── Help overlay ────────────────────────────────────────────────────────────

/// Built-in overlay that displays all registered hotkeys grouped by layer.
pub struct HelpOverlay<'a> {
    registry: &'a HotkeyRegistry,
}

impl<'a> HelpOverlay<'a> {
    pub fn new(registry: &'a HotkeyRegistry) -> Self {
        Self { registry }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let layers = self.registry.all_layers();

        // Measure content: each layer has a header line + one line per hotkey + blank line.
        let content_lines: usize = layers
            .iter()
            .map(|l| 1 + l.hotkeys.len() + 1)
            .sum::<usize>()
            .saturating_sub(1); // no trailing blank

        let padding: u16 = 1;
        let v_chrome = 2 + padding * 2; // border + vertical padding
        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = (content_lines as u16 + v_chrome).min(area.height.saturating_sub(2));

        if popup_width < 10 || popup_height < 4 {
            return;
        }

        let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup = Rect::new(popup_x, popup_y, popup_width, popup_height);

        // Clear popup area
        for y in popup.y..popup.y + popup.height {
            for x in popup.x..popup.x + popup.width {
                let cell = &mut buf[(x, y)];
                cell.set_char(' ');
                cell.set_style(Style::default());
            }
        }

        let content = HelpContent {
            registry: self.registry,
        };
        let bordered = Bordered::new(content)
            .title("Hotkeys")
            .bottom_item(Box::new(String::from("press any key to close")))
            .padding(1)
            .border_style(theme::HELP_BORDER);
        bordered.render(popup, buf);
    }
}
