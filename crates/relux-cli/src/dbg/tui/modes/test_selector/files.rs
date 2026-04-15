use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;

use super::TestSelectorState;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::filterable::FilterEvent;
use crate::dbg::tui::filterable::Filterable;
use crate::dbg::tui::filterable::Matchable;
use crate::dbg::tui::scrollable::Scrollable;
use crate::dbg::tui::theme;
use crate::dbg::tui::traits::BlockRenderable;
use crate::dbg::tui::traits::LineRenderable;
use crate::dbg::tui::traits::Listable;
use crate::dbg::tui::traits::MultilineRenderable;
use crate::dbg::tui::traits::Panel;
use crate::dbg::tui::traits::RenderKind;

// ── ReluxFile ─────────────────────────────────────────────────────────────

struct ReluxFile {
    path: PathBuf,
    /// Cached string form for fuzzy matching.
    text: String,
}

impl ReluxFile {
    fn new(path: PathBuf) -> Self {
        let text = path.to_string_lossy().to_string();
        Self { path, text }
    }
}

impl Matchable for ReluxFile {
    fn match_text(&self) -> &str {
        &self.text
    }
}

impl LineRenderable for ReluxFile {
    fn render(&self, _max_width: u16, _kind: RenderKind) -> Line<'static> {
        // Render path with directory components in DIR_NAME, filename in FILE_NAME.
        let text = &self.text;
        match text.rfind('/') {
            Some(pos) => {
                let dir_part = &text[..=pos];
                let file_part = &text[pos + 1..];
                Line::from(vec![
                    Span::styled(dir_part.to_string(), theme::DIR_NAME),
                    Span::styled(file_part.to_string(), theme::FILE_NAME),
                ])
            }
            None => Line::from(Span::styled(text.clone(), theme::FILE_NAME)),
        }
    }
}

// ── ReluxFileList ─────────────────────────────────────────────────────────

struct ReluxFileList {
    files: Vec<ReluxFile>,
}

impl ReluxFileList {
    fn new(files: Vec<ReluxFile>) -> Self {
        Self { files }
    }

    fn set_files(&mut self, files: Vec<ReluxFile>) {
        self.files = files;
    }
}

impl Listable for ReluxFileList {
    type Item<'a> = &'a ReluxFile;
    type Iter<'a> = std::slice::Iter<'a, ReluxFile>;

    fn iter(&self) -> Self::Iter<'_> {
        self.files.iter()
    }

    fn index(&self, index: usize) -> &ReluxFile {
        &self.files[index]
    }
}

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

impl MultilineRenderable for &TreeEntry {
    fn render_lines(&self, _max_width: u16) -> Vec<Line<'static>> {
        let indent = "  ".repeat(self.depth);
        let name_style = match self.kind {
            EntryKind::Directory => theme::DIR_NAME,
            EntryKind::File => theme::FILE_NAME,
        };
        vec![Line::from(vec![
            Span::styled(indent, name_style),
            Span::styled(self.display_name.clone(), name_style),
        ])]
    }

    fn line_count(&self, _max_width: u16) -> usize {
        1
    }
}

// ── FilesContent ───────────────────────────────────────────────────────────

struct FilesContent {
    entries: Vec<TreeEntry>,
}

impl Listable for FilesContent {
    type Item<'a> = &'a TreeEntry;
    type Iter<'a> = std::slice::Iter<'a, TreeEntry>;

    fn iter(&self) -> Self::Iter<'_> {
        self.entries.iter()
    }

    fn index(&self, index: usize) -> &TreeEntry {
        &self.entries[index]
    }
}

// ── FilesPanel ─────────────────────────────────────────────────────────────

pub struct FilesPanel {
    content: Scrollable<FilesContent>,
    tests_dir: PathBuf,
    filter: Scrollable<Filterable<ReluxFileList>>,
}

impl FilesPanel {
    const RELOAD: Hotkey = Hotkey::new('r', "reload");
    const FILTER: Hotkey = Hotkey::new('/', "filter");
    const FILTER_INPUT_WIDTH: u16 = 30;

    pub fn new(tests_dir: PathBuf) -> Self {
        let entries = build_tree(&tests_dir);
        let cursor = first_file(&entries).unwrap_or(0);
        let content = FilesContent { entries };
        let mut scrollable = Scrollable::new(content);
        scrollable.set_cursor(Some(cursor));

        let filter_items = discover_relux_files(&tests_dir);
        let filter_list = ReluxFileList::new(filter_items);
        let filter = Scrollable::new(Filterable::new(
            filter_list,
            Self::FILTER,
            Self::FILTER_INPUT_WIDTH,
        ));

        Self {
            content: scrollable,
            tests_dir,
            filter,
        }
    }

    fn reload(&mut self) {
        let cursor = self.content.cursor().unwrap_or(0);
        let inner = self.content.inner_mut();
        inner.entries = build_tree(&self.tests_dir);
        let new_cursor = nearest_file(&inner.entries, cursor).unwrap_or(0);
        self.content.set_cursor(Some(new_cursor));
        self.filter
            .inner_mut()
            .inner_mut()
            .set_files(discover_relux_files(&self.tests_dir));
        self.filter.clamp_cursor();
    }

    pub fn selected_file_path(&self) -> PathBuf {
        if self.filter.inner().is_active() {
            if let Some(cursor) = self.filter.cursor()
                && let Some(item) = self.filter.inner().selected(cursor)
            {
                return item.path.clone();
            }
            return PathBuf::new();
        }
        if let Some(cursor) = self.content.cursor() {
            let inner = self.content.inner();
            if let Some(entry) = inner.entries.get(cursor) {
                return entry.path.clone();
            }
        }
        PathBuf::new()
    }

    fn move_to_prev_file(&mut self) -> bool {
        if let Some(cursor) = self.content.cursor()
            && let Some(idx) = prev_file(&self.content.inner().entries, cursor)
        {
            self.content.set_cursor(Some(idx));
            return true;
        }
        false
    }

    fn move_to_next_file(&mut self) -> bool {
        if let Some(cursor) = self.content.cursor()
            && let Some(idx) = next_file(&self.content.inner().entries, cursor)
        {
            self.content.set_cursor(Some(idx));
            return true;
        }
        false
    }

    /// After filter deactivation, try to move the tree cursor to the file
    /// that was selected in the filter.
    fn restore_tree_cursor_from_filter(&mut self) {
        if let Some(cursor) = self.filter.cursor()
            && let Some(selected) = self.filter.inner().selected(cursor)
        {
            let path = &selected.path;
            let entries = &self.content.inner().entries;
            if let Some(idx) = entries
                .iter()
                .position(|e| matches!(e.kind, EntryKind::File) && e.path == *path)
            {
                self.content.set_cursor(Some(idx));
            }
        }
    }
}

impl BlockRenderable for FilesPanel {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.filter.inner().is_active() {
            self.filter.render(area, buf);
        } else {
            self.content.render(area, buf);
        }
    }
}

impl Panel<TestSelectorState> for FilesPanel {
    fn top_border_items(&self) -> Vec<Box<dyn LineRenderable>> {
        vec![
            Box::new("relux files".to_string()),
            self.filter.inner().border_item(),
        ]
    }

    fn hotkeys(&self) -> HotkeyLayer {
        if self.filter.inner().is_active() {
            return self.filter.inner().hotkey_layer();
        }
        HotkeyLayer::new("Files", vec![Self::RELOAD, Self::FILTER])
    }

    fn dispatch(&mut self, hotkey: &Hotkey, mode_state: &mut TestSelectorState) {
        if *hotkey == Self::RELOAD {
            self.reload();
            mode_state.selected_file = self.selected_file_path();
        } else if *hotkey == Self::FILTER {
            self.filter.inner_mut().activate();
            self.filter.set_cursor(Some(0));
        }
    }

    fn handle_key_event(&mut self, event: &KeyEvent, mode_state: &mut TestSelectorState) -> bool {
        if self.filter.inner().is_active() {
            return match event.code {
                KeyCode::Up => {
                    if let Some(c) = self.filter.cursor()
                        && c > 0
                    {
                        self.filter.set_cursor(Some(c - 1));
                        mode_state.selected_file = self.selected_file_path();
                        return true;
                    }
                    false
                }
                KeyCode::Down => {
                    if let Some(c) = self.filter.cursor() {
                        let len = self.filter.inner().iter().len();
                        if c + 1 < len {
                            self.filter.set_cursor(Some(c + 1));
                            mode_state.selected_file = self.selected_file_path();
                            return true;
                        }
                    }
                    false
                }
                _ => match self.filter.inner_mut().handle_key_event(event) {
                    FilterEvent::Changed => {
                        self.filter.clamp_cursor();
                        mode_state.selected_file = self.selected_file_path();
                        true
                    }
                    FilterEvent::Deactivated => {
                        self.restore_tree_cursor_from_filter();
                        self.filter.inner_mut().deactivate();
                        mode_state.selected_file = self.selected_file_path();
                        true
                    }
                    FilterEvent::Ignored => false,
                },
            };
        }

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

// ── File discovery ────────────────────────────────────────────────────────

fn discover_relux_files(tests_dir: &PathBuf) -> Vec<ReluxFile> {
    relux_core::discover::discover_relux_files(tests_dir)
        .into_iter()
        .filter_map(|f| {
            f.strip_prefix(tests_dir)
                .ok()
                .map(|r| ReluxFile::new(r.to_path_buf()))
        })
        .collect()
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
    entries
        .iter()
        .position(|e| matches!(e.kind, EntryKind::File))
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
