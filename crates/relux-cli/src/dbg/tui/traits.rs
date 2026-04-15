use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::core::Hotkey;
use super::core::hotkey_registry::HotkeyLayer;

// ── Render kind ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderKind {
    Active,
    Inactive,
}

// ── LineRenderable ─────────────────────────────────────────────────────────

pub trait LineRenderable {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static>;
}

impl<T: LineRenderable> LineRenderable for &T {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static> {
        (*self).render(max_width, kind)
    }
}

// ── MultilineRenderable ───────────────────────────────────────────────────

/// Renders an item into one or more styled terminal lines.
///
/// Mirrors [`LineRenderable`] for multi-line content. Each item is a single
/// logical entity that may occupy multiple display lines (e.g. a test with
/// a docstring, or a word-wrapped paragraph).
///
/// Used by [`super::scrollable::Scrollable`] to render visible items in a
/// scrolling viewport.
///
/// # Examples
///
/// Single-line item:
///
/// ```ignore
/// impl MultilineRenderable for &FileEntry {
///     fn render_lines(&self, _max_width: u16) -> Vec<Line<'static>> {
///         vec![Line::from(Span::styled(self.name.clone(), style::FILE_NAME))]
///     }
///
///     fn line_count(&self, _max_width: u16) -> usize { 1 }
/// }
/// ```
///
/// Multi-line item with word wrapping:
///
/// ```ignore
/// impl MultilineRenderable for &TestDetail {
///     fn render_lines(&self, max_width: u16) -> Vec<Line<'static>> {
///         let mut lines = Vec::new();
///         for subline in wrap_line(&self.name, max_width as usize) {
///             lines.push(Line::from(Span::styled(subline, style::TEST_NAME)));
///         }
///         lines
///     }
///
///     fn line_count(&self, max_width: u16) -> usize {
///         wrap_line(&self.name, max_width as usize).len()
///     }
/// }
/// ```
pub trait MultilineRenderable {
    /// Render this item into styled lines at the given width.
    fn render_lines(&self, max_width: u16) -> Vec<Line<'static>>;

    /// Number of lines this item will produce at the given width.
    /// Used by `Scrollable` for total line count (slider proportions)
    /// without allocating rendered lines.
    fn line_count(&self, max_width: u16) -> usize;
}

// ── Listable ──────────────────────────────────────────────────────────────

/// A thin collection trait: iterator + index access over items.
///
/// `Listable` is purely about data access — no rendering, no cursor.
/// Items know how to render themselves via [`MultilineRenderable`].
/// Cursor and viewport are managed by
/// [`Scrollable`](super::scrollable::Scrollable).
///
/// Uses GATs (Generic Associated Types) so that iterators can yield
/// borrowed or computed items tied to the collection's lifetime.
///
/// # Examples
///
/// Simple vec wrapper:
///
/// ```ignore
/// struct FileList { files: Vec<ReluxFile> }
///
/// impl Listable for FileList {
///     type Item<'a> = &'a ReluxFile;
///     type Iter<'a> = std::slice::Iter<'a, ReluxFile>;
///
///     fn iter(&self) -> Self::Iter<'_> { self.files.iter() }
///     fn index(&self, index: usize) -> &ReluxFile { &self.files[index] }
/// }
/// ```
///
/// Filtered view (yields computed items):
///
/// ```ignore
/// impl<L: Listable> Listable for Filterable<L> {
///     type Item<'a> = FilteredView<'a, ...> where Self: 'a;
///     type Iter<'a> = FilteredIter<'a, L> where Self: 'a;
///
///     fn iter(&self) -> Self::Iter<'_> { ... }
///     fn index(&self, index: usize) -> Self::Item<'_> { ... }
/// }
/// ```
pub trait Listable {
    type Item<'a>
    where
        Self: 'a;
    type Iter<'a>: ExactSizeIterator<Item = Self::Item<'a>>
    where
        Self: 'a;

    /// Iterate over all items. The iterator must implement
    /// `ExactSizeIterator` so that `Scrollable` can read `.len()`
    /// for viewport and slider computation before consuming items.
    fn iter(&self) -> Self::Iter<'_>;

    /// Access item by index. Panics if out of bounds, following the
    /// same contract as `std::ops::Index`.
    fn index(&self, index: usize) -> Self::Item<'_>;
}

// ── BlockRenderable ───────────────────────────────────────────────────────

pub trait BlockRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
}

impl<T: BlockRenderable> BlockRenderable for &T {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        (*self).render(area, buf);
    }
}

// ── Panel ───────────────────────────────────────────────────────────────────

pub trait Panel<S>: BlockRenderable {
    /// Items rendered in the top border (title, filter input, etc.).
    fn top_border_items(&self) -> Vec<Box<dyn LineRenderable>> {
        vec![]
    }

    /// Items rendered in the bottom border.
    fn bottom_border_items(&self) -> Vec<Box<dyn LineRenderable>> {
        vec![]
    }

    /// Hotkeys active when this panel is focused. Layer name = panel name.
    fn hotkeys(&self) -> HotkeyLayer;

    /// Handle a resolved hotkey. Only called for hotkeys from this panel's layer.
    fn dispatch(&mut self, hotkey: &Hotkey, mode_state: &mut S);

    /// Handle a raw key event (arrows, Enter, etc.).
    /// Returns `true` if mode state was mutated.
    fn handle_key_event(&mut self, _event: &KeyEvent, _mode_state: &mut S) -> bool {
        false
    }

    /// Called on sibling panels when another panel mutated the mode state.
    fn mode_state_changed(&mut self, _mode_state: &S) {}
}

// ── Mode ────────────────────────────────────────────────────────────────────

pub trait Mode {
    /// Render the mode's layout and panels.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Handle a mode-layer hotkey (panel focus switch).
    fn dispatch_focus(&mut self, hotkey: &Hotkey);

    /// Handle a panel-layer hotkey (forwarded to focused panel).
    fn dispatch_panel(&mut self, hotkey: &Hotkey);

    /// Forward a raw key event to the focused panel.
    fn forward_key_event(&mut self, event: &KeyEvent);

    /// Mode-level hotkeys (panel focus keys). Set once on mode enter.
    fn mode_hotkeys(&self) -> HotkeyLayer;

    /// Focused panel's hotkeys. Updated on focus change.
    fn panel_hotkeys(&self) -> HotkeyLayer;
}
