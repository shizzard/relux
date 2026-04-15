use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::theme;
use super::traits::BlockRenderable;
use super::traits::LineRenderable;
use super::traits::RenderKind;
use super::util::set_cell;

// ── Bordered ────────────────────────────────────────────────────────────────

pub struct Bordered<T: BlockRenderable> {
    panel: T,
    top_items: Vec<Box<dyn LineRenderable>>,
    bottom_items: Vec<Box<dyn LineRenderable>>,
    padding: u16,
    border_style: Option<Style>,
    focused: bool,
}

impl<T: BlockRenderable> Bordered<T> {
    pub fn new(panel: T) -> Self {
        Self {
            panel,
            top_items: Vec::new(),
            bottom_items: Vec::new(),
            padding: 0,
            border_style: None,
            focused: false,
        }
    }

    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.top_items.push(Box::new(text.into()));
        self
    }

    pub fn top_item(mut self, item: Box<dyn LineRenderable>) -> Self {
        self.top_items.push(item);
        self
    }

    pub fn bottom_item(mut self, item: Box<dyn LineRenderable>) -> Self {
        self.bottom_items.push(item);
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
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height < 2 {
            return;
        }

        let border_style = self.border_style.unwrap_or(if self.focused {
            theme::BORDER_FOCUSED
        } else {
            theme::BORDER
        });

        let kind = if self.focused {
            RenderKind::Active
        } else {
            RenderKind::Inactive
        };

        self.render_box(area, buf, border_style);
        self.render_top(area, buf, border_style, kind);
        self.render_bottom(area, buf, border_style, kind);

        // Render inner panel (border + padding).
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

    /// Top border items — left-aligned: ┌─┤item1├─┤item2├──────┐
    fn render_top(&self, area: Rect, buf: &mut Buffer, border_style: Style, kind: RenderKind) {
        if self.top_items.is_empty() {
            return;
        }
        let max_x = area.x + area.width.saturating_sub(2);
        let mut col = area.x + 2; // skip corner + 1 border char
        for item in &self.top_items {
            // Need at least 3 cols: ┤ + 1 char + ├
            if col + 2 >= max_x {
                break;
            }
            set_cell(col, area.y, '┤', border_style, buf);
            col += 1;
            let available = max_x.saturating_sub(col + 1); // reserve 1 for closing ├
            let line = item.render(available, kind);
            let (end_x, _) = buf.set_line(col, area.y, &line, available);
            col = end_x;
            set_cell(col, area.y, '├', border_style, buf);
            col += 2; // skip 1 border char gap before next item
        }
    }

    /// Bottom border items — right-aligned: └──────┤item1├─┤item2├─┘
    fn render_bottom(&self, area: Rect, buf: &mut Buffer, border_style: Style, kind: RenderKind) {
        if self.bottom_items.is_empty() {
            return;
        }
        let bottom_y = area.y + area.height - 1;
        let usable = area.width.saturating_sub(2) as usize; // inside the corners

        // Pre-render all items to measure total width.
        let max_per_item = usable.saturating_sub(self.bottom_items.len() * 2); // each needs ┤├
        let lines: Vec<_> = self
            .bottom_items
            .iter()
            .map(|item| item.render(max_per_item as u16, kind))
            .collect();

        // +2 for ┤├ per item, +1 gap after each item's ├
        let total: usize = lines.iter().map(|l| l.width() + 2 + 1).sum();
        if total == 0 {
            return;
        }

        let start_col = (area.x + area.width - 1).saturating_sub(total as u16);
        let mut col = start_col;
        for line in &lines {
            set_cell(col, bottom_y, '┤', border_style, buf);
            col += 1;
            let w = line.width() as u16;
            buf.set_line(col, bottom_y, line, w);
            col += w;
            set_cell(col, bottom_y, '├', border_style, buf);
            col += 2; // skip 1 border char gap
        }
    }
}
