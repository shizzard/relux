use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::theme;
use super::traits::BlockRenderable;
use super::util::set_cell;

// ── Scrollable ─────────────────────────────────────────────────────────────

/// Owns a `BlockRenderable` content struct and renders a scroll slider on the
/// right when the content overflows the visible area.
pub struct Scrollable<T> {
    inner: T,
    offset: usize,
    content_height: usize,
}

impl<T> Scrollable<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            offset: 0,
            content_height: 0,
        }
    }

    pub fn set_scroll(&mut self, offset: usize, content_height: usize) {
        self.offset = offset;
        self.content_height = content_height;
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: BlockRenderable> BlockRenderable for Scrollable<T> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let visible_height = area.height as usize;

        if self.content_height <= visible_height {
            self.inner.render(area, buf);
            return;
        }

        // Reserve 1 column on the right for the slider.
        let inner_area = Rect {
            width: area.width.saturating_sub(1),
            ..area
        };
        self.inner.render(inner_area, buf);

        render_slider(area, buf, self.offset, self.content_height, visible_height);
    }
}

// ── Slider rendering ───────────────────────────────────────────────────────

fn render_slider(
    area: Rect,
    buf: &mut Buffer,
    offset: usize,
    content_height: usize,
    visible_height: usize,
) {
    if content_height == 0 || visible_height == 0 {
        return;
    }

    let track_height = visible_height;
    let thumb_height = (visible_height * visible_height / content_height).max(1);
    let max_offset = content_height.saturating_sub(visible_height);
    let thumb_pos = if max_offset > 0 {
        offset * (track_height - thumb_height) / max_offset
    } else {
        0
    };

    let slider_x = area.x + area.width - 1;
    for row in 0..track_height {
        let ch = if row >= thumb_pos && row < thumb_pos + thumb_height {
            '█'
        } else {
            '░'
        };
        set_cell(slider_x, area.y + row as u16, ch, theme::BORDER, buf);
    }
}
