use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// A content widget that fills a rectangular area.
/// Leaf-level: source viewer, tree view, variable list, shell buffer, status bar.
pub trait Panel {
    fn render(&self, area: Rect, buf: &mut Buffer);
}

/// A layout that arranges panels into a full-screen mode.
/// One per UI mode: test selector, pre-run, execution.
pub trait Screen {
    fn render(&self, area: Rect, buf: &mut Buffer);
}

/// A floating layer rendered on top of a screen.
/// Popups: eval log overlay, function picker, shell switcher, effects, help.
pub trait Overlay {
    fn render(&self, area: Rect, buf: &mut Buffer);
}
