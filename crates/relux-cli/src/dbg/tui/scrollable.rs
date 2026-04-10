use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::traits::Panel;

/// Wraps a `Panel` that may produce content taller than its visible area.
/// Renders the visible window and an optional scroll indicator.
pub struct Scrollable<P: Panel> {
    panel: P,
    offset: usize,
    content_height: usize,
}

impl<P: Panel> Scrollable<P> {
    pub fn new(panel: P, content_height: usize) -> Self {
        Self {
            panel,
            offset: 0,
            content_height,
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_offset(&mut self, offset: usize) {
        let max = self.content_height.saturating_sub(1);
        self.offset = offset.min(max);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.set_offset(self.offset.saturating_add(lines));
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.set_offset(self.offset.saturating_sub(lines));
    }

    pub fn panel(&self) -> &P {
        &self.panel
    }

    pub fn panel_mut(&mut self) -> &mut P {
        &mut self.panel
    }
}

impl<P: Panel> Panel for Scrollable<P> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let visible_height = area.height as usize;

        if self.content_height <= visible_height {
            // No scrolling needed — render directly.
            self.panel.render(area, buf);
            return;
        }

        // Render into an oversized buffer, then copy the visible window.
        let full_area = Rect {
            x: 0,
            y: 0,
            width: area.width,
            height: self.content_height as u16,
        };
        let mut scratch = Buffer::empty(full_area);
        self.panel.render(full_area, &mut scratch);

        // Copy visible rows from scratch into the real buffer.
        let offset = self
            .offset
            .min(self.content_height.saturating_sub(visible_height));
        for row in 0..visible_height {
            let src_y = (offset + row) as u16;
            if src_y >= self.content_height as u16 {
                break;
            }
            for col in 0..area.width {
                let src_cell = &scratch[(col, src_y)];
                let dst = &mut buf[(area.x + col, area.y + row as u16)];
                dst.set_symbol(src_cell.symbol());
                dst.set_style(src_cell.style());
            }
        }

        // Scroll indicator on the right edge.
        self.render_scroll_indicator(area, buf, offset, visible_height);
    }
}

impl<P: Panel> Scrollable<P> {
    fn render_scroll_indicator(
        &self,
        area: Rect,
        buf: &mut Buffer,
        offset: usize,
        visible_height: usize,
    ) {
        if self.content_height == 0 || visible_height == 0 {
            return;
        }
        let track_height = visible_height;
        let thumb_height = (visible_height * visible_height / self.content_height).max(1);
        let max_offset = self.content_height.saturating_sub(visible_height);
        let thumb_pos = if max_offset > 0 {
            offset * (track_height - thumb_height) / max_offset
        } else {
            0
        };

        let indicator_x = area.x + area.width - 1;
        for row in 0..track_height {
            let ch = if row >= thumb_pos && row < thumb_pos + thumb_height {
                '█'
            } else {
                '░'
            };
            let cell = &mut buf[(indicator_x, area.y + row as u16)];
            cell.set_char(ch);
        }
    }
}
