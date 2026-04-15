use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;

use super::theme;
use super::traits::BlockRenderable;
use super::traits::Listable;
use super::traits::MultilineRenderable;
use super::util::set_cell;

// ── Scrollable ─────────────────────────────────────────────────────────────

/// Owns a `Listable` content struct and renders visible items in a scrolling
/// viewport with a cursor marker and scroll slider.
///
/// Cursor is owned by `Scrollable`, not by the inner content. Panels handle
/// navigation logic (e.g. skipping directories) and call `set_cursor()`.
pub struct Scrollable<T> {
    inner: T,
    cursor: Option<usize>,
}

impl<T> Scrollable<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cursor: None,
        }
    }

    pub fn set_cursor(&mut self, cursor: Option<usize>) {
        self.cursor = cursor;
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: Listable> Scrollable<T> {
    /// Clamp cursor to valid range. Call after the inner content changes
    /// (e.g. filter recomputation, reload). Resets to 0 if out of bounds,
    /// or `None` if the list is empty.
    pub fn clamp_cursor(&mut self) {
        let len = self.inner.iter().len();
        match self.cursor {
            Some(c) if c >= len => {
                self.cursor = if len > 0 { Some(len - 1) } else { None };
            }
            _ => {}
        }
    }
}

impl<T: Listable> BlockRenderable for Scrollable<T>
where
    for<'a> T::Item<'a>: MultilineRenderable,
{
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let visible = area.height as usize;
        let item_width = area.width.saturating_sub(2); // cursor marker

        // Clamp cursor: if item count changed and cursor is out of bounds,
        // treat it as 0 (or None if empty).
        let item_count = self.inner.iter().len();
        let cursor = match self.cursor {
            Some(c) if c >= item_count && item_count > 0 => Some(item_count - 1),
            Some(_) if item_count == 0 => None,
            other => other,
        };

        // First pass: compute total line count and cursor line offset.
        // This iterates all items before rendering — acceptable for slice
        // iterators (O(n) with no allocation). Can be optimized to single
        // pass if profiling shows it matters.
        let mut total_lines = 0usize;
        let mut cursor_line = 0usize;
        for (idx, item) in self.inner.iter().enumerate() {
            if cursor == Some(idx) {
                cursor_line = total_lines;
            }
            total_lines += item.line_count(item_width);
        }

        // Reserve 1 col for slider if overflowing.
        let content_width = if total_lines > visible {
            area.width.saturating_sub(1)
        } else {
            area.width
        };
        let item_width = content_width.saturating_sub(2);

        // Compute line-based scroll offset (center cursor in viewport).
        let offset = match cursor {
            Some(_) => {
                let center = visible / 2;
                let max_scroll = total_lines.saturating_sub(visible);
                cursor_line.saturating_sub(center).min(max_scroll)
            }
            None => 0,
        };

        // Second pass: render visible lines.
        let mut line_idx = 0usize;
        let mut row = 0u16;
        for (idx, item) in self.inner.iter().enumerate() {
            let lines = item.render_lines(item_width);
            let is_cursor = cursor == Some(idx);

            for (i, line) in lines.iter().enumerate() {
                if line_idx >= offset {
                    if row >= area.height {
                        break;
                    }
                    let marker = if i == 0 && is_cursor { "► " } else { "  " };
                    let mut spans = vec![Span::styled(marker.to_string(), theme::FILE_CURSOR)];
                    spans.extend(line.spans.clone());
                    buf.set_line(area.x, area.y + row, &Line::from(spans), content_width);
                    row += 1;
                }
                line_idx += 1;
            }
            if row >= area.height {
                break;
            }
        }

        if total_lines > visible {
            render_slider(area, buf, offset, total_lines, visible);
        }
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
