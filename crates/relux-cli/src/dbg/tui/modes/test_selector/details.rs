use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;

use super::TestSelectorState;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::Label;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::traits::Panel;
use crate::dbg::tui::scrollable::Scrollable;
use crate::dbg::tui::theme;
use crate::dbg::tui::traits::BlockRenderable;

// ── Test info ──────────────────────────────────────────────────────────────

struct TestInfo {
    name: String,
    docstring: Option<String>,
}

/// Wrap a string into lines that fit within `max_width` characters.
/// Breaks on word boundaries when possible, hard-breaks long words.
fn wrap_line(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![];
    }
    if text.len() <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            if word.len() > max_width {
                // Hard-break long words.
                let mut remaining = word;
                while remaining.len() > max_width {
                    let (chunk, rest) = remaining.split_at(max_width);
                    lines.push(chunk.to_string());
                    remaining = rest;
                }
                current = remaining.to_string();
            } else {
                current = word.to_string();
            }
        } else if current.len() + 1 + word.len() <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            if word.len() > max_width {
                let mut remaining = word;
                while remaining.len() > max_width {
                    let (chunk, rest) = remaining.split_at(max_width);
                    lines.push(chunk.to_string());
                    remaining = rest;
                }
                current = remaining.to_string();
            } else {
                current = word.to_string();
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Reformat a docstring for display: single newlines become spaces,
/// two or more consecutive newlines become a single line break.
fn reformat_docstring(doc: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    for line in doc.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.clone());
                current.clear();
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line.trim());
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    paragraphs
}

// ── DetailsContent ─────────────────────────────────────────────────────────

struct DetailsContent {
    tests: Vec<TestInfo>,
    last_total_lines: std::cell::Cell<usize>,
}

impl DetailsContent {
    fn total_lines(&self, width: usize) -> usize {
        self.tests
            .iter()
            .map(|t| {
                let name = format!("test \"{}\"", t.name);
                let name_lines = wrap_line(&name, width).len();
                let doc_lines = t
                    .docstring
                    .as_ref()
                    .map(|d| {
                        let doc_width = width.saturating_sub(2); // "  " indent
                        reformat_docstring(d)
                            .iter()
                            .map(|p| wrap_line(p, doc_width).len())
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                name_lines + doc_lines
            })
            .sum()
    }
}

impl BlockRenderable for DetailsContent {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if self.tests.is_empty() {
            let hint = Line::from(Span::styled("no file selected", theme::HINT));
            buf.set_line(area.x, area.y, &hint, area.width);
            return;
        }

        let width = area.width as usize;
        self.last_total_lines.set(self.total_lines(width));

        let mut row = 0u16;
        for test in &self.tests {
            if row >= area.height {
                break;
            }

            let name = format!("test \"{}\"", test.name);
            for subline in wrap_line(&name, width) {
                if row >= area.height {
                    break;
                }
                let line = Line::from(Span::styled(subline, theme::TEST_NAME));
                buf.set_line(area.x, area.y + row, &line, area.width);
                row += 1;
            }

            if let Some(doc) = &test.docstring {
                let doc_width = width.saturating_sub(2);
                for paragraph in reformat_docstring(doc) {
                    for subline in wrap_line(&paragraph, doc_width) {
                        if row >= area.height {
                            break;
                        }
                        let line = Line::from(Span::styled(
                            format!("  {subline}"),
                            theme::TEST_DOCSTRING,
                        ));
                        buf.set_line(area.x, area.y + row, &line, area.width);
                        row += 1;
                    }
                }
            }
        }
    }
}

// ── DetailsPanel ───────────────────────────────────────────────────────────

pub struct DetailsPanel {
    content: Scrollable<DetailsContent>,
    tests_dir: PathBuf,
}

impl DetailsPanel {
    pub fn new(tests_dir: PathBuf) -> Self {
        Self {
            content: Scrollable::new(DetailsContent {
                tests: Vec::new(),
                last_total_lines: std::cell::Cell::new(0),
            }),
            tests_dir,
        }
    }

    fn load_file(&mut self, relative_path: &PathBuf) {
        let abs_path = self.tests_dir.join(relative_path);
        let source = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(_) => {
                self.content.inner_mut().tests.clear();
                self.content.set_scroll(0, 0);
                return;
            }
        };

        let module = match relux_parser::parse(&source) {
            Ok(m) => m,
            Err(_) => {
                self.content.inner_mut().tests.clear();
                self.content.set_scroll(0, 0);
                return;
            }
        };

        let inner = self.content.inner_mut();
        inner.tests = module
            .items
            .iter()
            .filter_map(|item| {
                if let relux_ast::AstItem::Test { def, .. } = &item.node {
                    let docstring = def.body.iter().find_map(|item| {
                        if let relux_ast::AstTestItem::DocString { text, .. } = &item.node {
                            Some(text.clone())
                        } else {
                            None
                        }
                    });
                    Some(TestInfo {
                        name: def.name.node.clone(),
                        docstring,
                    })
                } else {
                    None
                }
            })
            .collect();

        let total = inner.last_total_lines.get();
        self.content.set_scroll(0, total);
    }
}

impl BlockRenderable for DetailsPanel {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.content.render(area, buf);
    }
}

impl Panel<TestSelectorState> for DetailsPanel {
    fn label(&self) -> Label {
        Label::bare("details")
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Details", vec![])
    }

    fn dispatch(&mut self, _hotkey: &Hotkey, _mode_state: &mut TestSelectorState) {}

    fn mode_state_changed(&mut self, mode_state: &TestSelectorState) {
        self.load_file(&mode_state.selected_file);
    }
}
