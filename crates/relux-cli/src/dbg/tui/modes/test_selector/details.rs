use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;

use super::TestSelectorState;
use crate::dbg::tui::core::Hotkey;
use crate::dbg::tui::core::hotkey_registry::HotkeyLayer;
use crate::dbg::tui::scrollable::Scrollable;
use crate::dbg::tui::theme;
use crate::dbg::tui::traits::BlockRenderable;
use crate::dbg::tui::traits::LineRenderable;
use crate::dbg::tui::traits::Listable;
use crate::dbg::tui::traits::MultilineRenderable;
use crate::dbg::tui::traits::Panel;

// ── TestDetail ────────────────────────────────────────────────────────────

struct TestDetail {
    name: String,
    docstring: Option<String>,
}

impl MultilineRenderable for &TestDetail {
    fn render_lines(&self, max_width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let width = max_width as usize;

        // Test name.
        let name = format!("test \"{}\"", self.name);
        for subline in wrap_line(&name, width) {
            lines.push(Line::from(Span::styled(subline, theme::TEST_NAME)));
        }

        // Docstring paragraphs (indented).
        if let Some(doc) = &self.docstring {
            let doc_width = width.saturating_sub(2);
            for paragraph in reformat_docstring(doc) {
                for subline in wrap_line(&paragraph, doc_width) {
                    lines.push(Line::from(Span::styled(
                        format!("  {subline}"),
                        theme::TEST_DOCSTRING,
                    )));
                }
            }
        }

        lines
    }

    fn line_count(&self, max_width: u16) -> usize {
        let width = max_width as usize;
        let name = format!("test \"{}\"", self.name);
        let mut count = wrap_line(&name, width).len();
        if let Some(doc) = &self.docstring {
            let doc_width = width.saturating_sub(2);
            for paragraph in reformat_docstring(doc) {
                count += wrap_line(&paragraph, doc_width).len();
            }
        }
        count
    }
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
    tests: Vec<TestDetail>,
}

impl Listable for DetailsContent {
    type Item<'a> = &'a TestDetail;
    type Iter<'a> = std::slice::Iter<'a, TestDetail>;

    fn iter(&self) -> Self::Iter<'_> {
        self.tests.iter()
    }

    fn index(&self, index: usize) -> &TestDetail {
        &self.tests[index]
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
            content: Scrollable::new(DetailsContent { tests: Vec::new() }),
            tests_dir,
        }
    }

    fn load_file(&mut self, relative_path: &PathBuf) {
        let abs_path = self.tests_dir.join(relative_path);
        let source = match std::fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(_) => {
                self.content.inner_mut().tests.clear();
                return;
            }
        };

        let module = match relux_parser::parse(&source) {
            Ok(m) => m,
            Err(_) => {
                self.content.inner_mut().tests.clear();
                return;
            }
        };

        self.content.inner_mut().tests = module
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
                    Some(TestDetail {
                        name: def.name.node.clone(),
                        docstring,
                    })
                } else {
                    None
                }
            })
            .collect();
    }
}

impl BlockRenderable for DetailsPanel {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self.content.inner().tests.is_empty() {
            let hint = Line::from(Span::styled("no file selected", theme::HINT));
            buf.set_line(area.x, area.y, &hint, area.width);
            return;
        }
        self.content.render(area, buf);
    }
}

impl Panel<TestSelectorState> for DetailsPanel {
    fn top_border_items(&self) -> Vec<Box<dyn LineRenderable>> {
        vec![Box::new("details".to_string())]
    }

    fn hotkeys(&self) -> HotkeyLayer {
        HotkeyLayer::new("Details", vec![])
    }

    fn dispatch(&mut self, _hotkey: &Hotkey, _mode_state: &mut TestSelectorState) {}

    fn mode_state_changed(&mut self, mode_state: &TestSelectorState) {
        self.load_file(&mode_state.selected_file);
    }
}
