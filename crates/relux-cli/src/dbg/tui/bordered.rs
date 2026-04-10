use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::hotkey::Hotkey;
use super::hotkey::HotkeyRegistry;
use super::theme;
use super::traits::Panel;
use super::util::set_cell;

// ── Border items ────────────────────────────────────────────────────────────

pub enum BorderItem {
    Title {
        text: String,
        hotkey: Option<Hotkey>,
    },
    Input {
        hotkey: Hotkey,
        value: String,
        editing: bool,
    },
}

impl BorderItem {
    pub fn title(text: impl Into<String>) -> Self {
        Self::Title {
            text: text.into(),
            hotkey: None,
        }
    }

    pub fn title_with_hotkey(hotkey: Hotkey) -> Self {
        Self::Title {
            text: hotkey.label.clone(),
            hotkey: Some(hotkey),
        }
    }

    pub fn input(hotkey: Hotkey) -> Self {
        Self::Input {
            hotkey,
            value: String::new(),
            editing: false,
        }
    }

    /// Measure the rendered width of this item (without writing).
    fn measure(&self, registry: &HotkeyRegistry) -> u16 {
        match self {
            Self::Title { text, hotkey } => {
                let base_len = text.chars().count() as u16;
                if let Some(hk) = hotkey {
                    let active = registry.is_active(hk.key);
                    let key_in_label = text.chars().any(|c| c.eq_ignore_ascii_case(&hk.key));
                    if active && !key_in_label {
                        1 + base_len
                    } else {
                        base_len
                    }
                } else {
                    base_len
                }
            }
            Self::Input {
                hotkey,
                value,
                editing,
            } => {
                if *editing {
                    let label_len = hotkey.label.chars().count() as u16;
                    label_len + 1 + value.chars().count() as u16 + 1
                } else {
                    let active = registry.is_active(hotkey.key);
                    let key_in_label = hotkey
                        .label
                        .chars()
                        .any(|c| c.eq_ignore_ascii_case(&hotkey.key));
                    if active && !key_in_label {
                        1 + hotkey.label.chars().count() as u16
                    } else {
                        hotkey.label.chars().count() as u16
                    }
                }
            }
        }
    }

    /// Render this item at the given position, returning columns consumed.
    fn render(
        &self,
        x: u16,
        y: u16,
        buf: &mut Buffer,
        focused: bool,
        registry: &HotkeyRegistry,
    ) -> u16 {
        match self {
            Self::Title { text, hotkey } => match hotkey {
                Some(hk) => {
                    let active = registry.is_active(hk.key);
                    hk.render_label(x, y, buf, active)
                }
                None => {
                    let style = if focused {
                        theme::TITLE
                    } else {
                        theme::HOTKEY_INACTIVE
                    };
                    let mut col = x;
                    for ch in text.chars() {
                        set_cell(col, y, ch, style, buf);
                        col += 1;
                    }
                    col - x
                }
            },
            Self::Input {
                hotkey,
                value,
                editing,
            } => {
                if *editing {
                    let active = registry.is_active(hotkey.key);
                    let mut col = x;
                    col += hotkey.render_label(col, y, buf, active);
                    set_cell(col, y, ':', theme::INPUT_EDITING, buf);
                    col += 1;
                    for ch in value.chars() {
                        set_cell(col, y, ch, theme::INPUT_EDITING, buf);
                        col += 1;
                    }
                    set_cell(col, y, '█', theme::INPUT_EDITING, buf);
                    col += 1;
                    col - x
                } else {
                    let active = registry.is_active(hotkey.key);
                    hotkey.render_label(x, y, buf, active)
                }
            }
        }
    }
}

// ── Bordered panel ──────────────────────────────────────────────────────────

/// Static text rendered centered on the bottom border.
struct BottomHint {
    text: String,
    style: Style,
}

pub struct Bordered<P: Panel> {
    panel: P,
    left_items: Vec<BorderItem>,
    right_items: Vec<BorderItem>,
    bottom_hint: Option<BottomHint>,
    padding: u16,
    border_style: Option<Style>,
    focused: bool,
}

impl<P: Panel> Bordered<P> {
    pub fn new(panel: P) -> Self {
        Self {
            panel,
            left_items: Vec::new(),
            right_items: Vec::new(),
            bottom_hint: None,
            padding: 0,
            border_style: None,
            focused: false,
        }
    }

    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.left_items.push(BorderItem::title(text));
        self
    }

    pub fn left_item(mut self, item: BorderItem) -> Self {
        self.left_items.push(item);
        self
    }

    pub fn right_item(mut self, item: BorderItem) -> Self {
        self.right_items.push(item);
        self
    }

    pub fn bottom_hint(mut self, text: impl Into<String>, style: Style) -> Self {
        self.bottom_hint = Some(BottomHint {
            text: text.into(),
            style,
        });
        self
    }

    pub fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = Some(style);
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn right_items_mut(&mut self) -> &mut Vec<BorderItem> {
        &mut self.right_items
    }

    /// Compute the inner area (content area inside the border, without padding).
    pub fn inner(area: Rect) -> Rect {
        if area.width < 2 || area.height < 2 {
            return Rect::default();
        }
        Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width - 2,
            height: area.height - 2,
        }
    }

    /// Render the bordered panel.
    pub fn render(&self, area: Rect, buf: &mut Buffer, registry: &HotkeyRegistry) {
        if area.width < 2 || area.height < 2 {
            return;
        }

        let border_style = self.border_style.unwrap_or(if self.focused {
            theme::BORDER_FOCUSED
        } else {
            theme::BORDER
        });

        // Draw box-drawing border
        self.render_box(area, buf, border_style);

        // Render left items on top border: ┌┤ title ├──┤ title2 ├───...
        self.render_top_left(area, buf, border_style, registry);

        // Render right items on top border (right-aligned)
        self.render_top_right(area, buf, border_style, registry);

        // Render bottom hint (centered on bottom border)
        if let Some(hint) = &self.bottom_hint {
            self.render_bottom_center(area, buf, border_style, &hint.text, hint.style);
        }

        // Render inner panel (border + padding)
        let inner = Self::inner(area);
        let h_padding = self.padding * 2; // compensate for ~1:2 cell aspect ratio
        let padded = Rect {
            x: inner.x + h_padding,
            y: inner.y + self.padding,
            width: inner.width.saturating_sub(h_padding * 2),
            height: inner.height.saturating_sub(self.padding * 2),
        };
        if padded.width > 0 && padded.height > 0 {
            self.panel.render(padded, buf);
        }
    }

    fn render_box(&self, area: Rect, buf: &mut Buffer, style: Style) {
        let x1 = area.x;
        let y1 = area.y;
        let x2 = area.x + area.width - 1;
        let y2 = area.y + area.height - 1;

        set_cell(x1, y1, '┌', style, buf);
        set_cell(x2, y1, '┐', style, buf);
        set_cell(x1, y2, '└', style, buf);
        set_cell(x2, y2, '┘', style, buf);

        for x in (x1 + 1)..x2 {
            set_cell(x, y1, '─', style, buf);
            set_cell(x, y2, '─', style, buf);
        }
        for y in (y1 + 1)..y2 {
            set_cell(x1, y, '│', style, buf);
            set_cell(x2, y, '│', style, buf);
        }
    }

    fn render_top_left(
        &self,
        area: Rect,
        buf: &mut Buffer,
        border_style: Style,
        registry: &HotkeyRegistry,
    ) {
        let max_x = area.x + area.width.saturating_sub(2);
        let mut col = area.x + 1;
        for item in &self.left_items {
            if col >= max_x {
                break;
            }
            set_cell(col, area.y, '┤', border_style, buf);
            col += 1;
            set_cell(col, area.y, ' ', border_style, buf);
            col += 1;
            col += item.render(col, area.y, buf, self.focused, registry);
            set_cell(col, area.y, ' ', border_style, buf);
            col += 1;
            set_cell(col, area.y, '├', border_style, buf);
            col += 1;
        }
    }

    fn render_top_right(
        &self,
        area: Rect,
        buf: &mut Buffer,
        border_style: Style,
        registry: &HotkeyRegistry,
    ) {
        if self.right_items.is_empty() {
            return;
        }
        let total_right: u16 = self
            .right_items
            .iter()
            .map(|item| item.measure(registry) + 4)
            .sum();

        let right_start = (area.x + area.width).saturating_sub(1 + total_right);
        let mut rcol = right_start;
        for item in &self.right_items {
            if rcol >= area.x + area.width - 1 {
                break;
            }
            set_cell(rcol, area.y, '┤', border_style, buf);
            rcol += 1;
            set_cell(rcol, area.y, ' ', border_style, buf);
            rcol += 1;
            rcol += item.render(rcol, area.y, buf, self.focused, registry);
            set_cell(rcol, area.y, ' ', border_style, buf);
            rcol += 1;
            set_cell(rcol, area.y, '├', border_style, buf);
            rcol += 1;
        }
    }

    fn render_bottom_center(
        &self,
        area: Rect,
        buf: &mut Buffer,
        border_style: Style,
        text: &str,
        text_style: Style,
    ) {
        let bottom_y = area.y + area.height - 1;
        let total_w = text.chars().count() as u16 + 4;
        let start_x = area.x + (area.width.saturating_sub(total_w)) / 2;

        set_cell(start_x, bottom_y, '┤', border_style, buf);
        set_cell(start_x + 1, bottom_y, ' ', border_style, buf);
        let mut col = start_x + 2;
        for ch in text.chars() {
            set_cell(col, bottom_y, ch, text_style, buf);
            col += 1;
        }
        set_cell(col, bottom_y, ' ', border_style, buf);
        set_cell(col + 1, bottom_y, '├', border_style, buf);
    }
}
