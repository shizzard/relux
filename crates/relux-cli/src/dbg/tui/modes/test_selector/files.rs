use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;

use super::TestSelectorState;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::Label;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::panel::Panel;
use crate::dbg::tui::scrollable::Scrollable;
use crate::dbg::tui::theme;
use crate::dbg::tui::traits::BlockRenderable;

// ── Tree entry ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug)]
struct TreeEntry {
    kind: EntryKind,
    depth: usize,
    display_name: String,
    /// For files: the relative path from tests_dir. For directories: the dir prefix.
    path: PathBuf,
}

// ── FilesContent ───────────────────────────────────────────────────────────

struct FilesContent {
    entries: Vec<TreeEntry>,
    cursor: usize,
    last_offset: std::cell::Cell<usize>,
}

impl BlockRenderable for FilesContent {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let visible_height = area.height as usize;
        let scroll_offset = self.compute_scroll_offset(visible_height);
        self.last_offset.set(scroll_offset);
        let end = (scroll_offset + visible_height).min(self.entries.len());

        for (row, idx) in (scroll_offset..end).enumerate() {
            let entry = &self.entries[idx];
            let is_cursor = idx == self.cursor;
            let indent = "  ".repeat(entry.depth);

            let cursor_marker = if is_cursor { "► " } else { "  " };

            let name_style = match entry.kind {
                EntryKind::Directory => theme::DIR_NAME,
                EntryKind::File => theme::FILE_NAME,
            };

            let line = Line::from(vec![
                Span::styled(cursor_marker.to_string(), theme::FILE_CURSOR),
                Span::styled(indent, name_style),
                Span::styled(entry.display_name.clone(), name_style),
            ]);

            buf.set_line(area.x, area.y + row as u16, &line, area.width);
        }
    }
}

impl FilesContent {
    fn compute_scroll_offset(&self, visible_height: usize) -> usize {
        let center = visible_height / 2;
        let max_scroll = self.entries.len().saturating_sub(visible_height);
        self.cursor.saturating_sub(center).min(max_scroll)
    }
}

// ── FilesPanel ─────────────────────────────────────────────────────────────

pub struct FilesPanel {
    content: Scrollable<FilesContent>,
    tests_dir: PathBuf,
}

impl FilesPanel {
    const RELOAD: Hotkey = Hotkey::new('r', "reload");

    pub fn new(tests_dir: PathBuf) -> Self {
        let entries = build_tree(&tests_dir);
        let cursor = first_file(&entries).unwrap_or(0);
        let content = FilesContent {
            entries,
            cursor,
            last_offset: std::cell::Cell::new(0),
        };
        let mut scrollable = Scrollable::new(content);
        Self::update_scroll(&mut scrollable);
        Self {
            content: scrollable,
            tests_dir,
        }
    }

    fn reload(&mut self) {
        let inner = self.content.inner_mut();
        inner.entries = build_tree(&self.tests_dir);
        inner.cursor = nearest_file(&inner.entries, inner.cursor).unwrap_or(0);
        Self::update_scroll(&mut self.content);
    }

    pub fn selected_file_path(&self) -> PathBuf {
        let inner = self.content.inner();
        inner
            .entries
            .get(inner.cursor)
            .map(|e| e.path.clone())
            .unwrap_or_default()
    }

    fn move_to_prev_file(&mut self) -> bool {
        let inner = self.content.inner_mut();
        if let Some(idx) = prev_file(&inner.entries, inner.cursor) {
            inner.cursor = idx;
            Self::update_scroll(&mut self.content);
            return true;
        }
        false
    }

    fn move_to_next_file(&mut self) -> bool {
        let inner = self.content.inner_mut();
        if let Some(idx) = next_file(&inner.entries, inner.cursor) {
            inner.cursor = idx;
            Self::update_scroll(&mut self.content);
            return true;
        }
        false
    }

    fn update_scroll(scrollable: &mut Scrollable<FilesContent>) {
        let inner = scrollable.inner();
        let content_height = inner.entries.len();
        let offset = inner.last_offset.get();
        scrollable.set_scroll(offset, content_height);
    }
}

impl BlockRenderable for FilesPanel {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // Update scroll offset with actual visible height before rendering.
        // We can't mutate self, but the Scrollable render uses its stored offset
        // for the slider. The content render computes its own offset from cursor.
        // We need them in sync — so we accept a minor mismatch on the first frame
        // after a resize, which self-corrects on the next key event.
        self.content.render(area, buf);
    }
}

impl Panel<TestSelectorState> for FilesPanel {
    fn label(&self) -> Label {
        Label::bare("relux files")
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Files", vec![Self::RELOAD])
    }

    fn dispatch(&mut self, hotkey: &Hotkey, mode_state: &mut TestSelectorState) {
        if *hotkey == Self::RELOAD {
            self.reload();
            mode_state.selected_file = self.selected_file_path();
        }
    }

    fn handle_key_event(
        &mut self,
        event: &KeyEvent,
        mode_state: &mut TestSelectorState,
    ) -> bool {
        let moved = match event.code {
            KeyCode::Up => self.move_to_prev_file(),
            KeyCode::Down => self.move_to_next_file(),
            _ => false,
        };
        if moved {
            mode_state.selected_file = self.selected_file_path();
        }
        moved
    }
}

// ── Tree builder ───────────────────────────────────────────────────────────

fn build_tree(tests_dir: &PathBuf) -> Vec<TreeEntry> {
    let files = relux_core::discover::discover_relux_files(tests_dir);

    let mut relative_paths: Vec<PathBuf> = files
        .into_iter()
        .filter_map(|f| f.strip_prefix(tests_dir).ok().map(|r| r.to_path_buf()))
        .collect();
    relative_paths.sort();

    let mut entries = Vec::new();
    let mut emitted_dirs: Vec<Vec<String>> = Vec::new();

    for rel_path in &relative_paths {
        let components: Vec<String> = rel_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();

        // Emit directory entries for any new directory levels.
        for depth in 0..components.len().saturating_sub(1) {
            let dir_components = &components[..=depth];
            if !emitted_dirs.iter().any(|d| d == dir_components) {
                entries.push(TreeEntry {
                    kind: EntryKind::Directory,
                    depth,
                    display_name: format!("{}/", components[depth]),
                    path: PathBuf::from_iter(dir_components),
                });
                emitted_dirs.push(dir_components.to_vec());
            }
        }

        // Emit the file entry.
        let file_depth = components.len().saturating_sub(1);
        let file_name = components.last().unwrap().clone();
        entries.push(TreeEntry {
            kind: EntryKind::File,
            depth: file_depth,
            display_name: file_name,
            path: rel_path.clone(),
        });
    }

    entries
}

fn is_file(entries: &[TreeEntry], idx: usize) -> bool {
    matches!(entries.get(idx), Some(e) if matches!(e.kind, EntryKind::File))
}

fn first_file(entries: &[TreeEntry]) -> Option<usize> {
    entries.iter().position(|e| matches!(e.kind, EntryKind::File))
}

fn next_file(entries: &[TreeEntry], from: usize) -> Option<usize> {
    ((from + 1)..entries.len()).find(|&i| is_file(entries, i))
}

fn prev_file(entries: &[TreeEntry], from: usize) -> Option<usize> {
    (0..from).rev().find(|&i| is_file(entries, i))
}

fn nearest_file(entries: &[TreeEntry], from: usize) -> Option<usize> {
    if is_file(entries, from) {
        return Some(from);
    }
    next_file(entries, from).or_else(|| prev_file(entries, from))
}
