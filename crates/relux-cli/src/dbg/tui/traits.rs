use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

// ── Render kind ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderKind {
    Active,
    Inactive,
}

// ── Traits ──────────────────────────────────────────────────────────────────

pub trait LineRenderable {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static>;
}

pub trait BlockRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
}

impl<T: BlockRenderable> BlockRenderable for &T {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        (*self).render(area, buf);
    }
}
