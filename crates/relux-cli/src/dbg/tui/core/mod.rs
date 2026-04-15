pub mod hotkey_registry;
pub use hotkey_registry::*;

use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::theme;
use super::traits::LineRenderable;
use super::traits::RenderKind;

// ── Hotkey ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub key: char,
    pub description: &'static str,
}

impl Hotkey {
    pub const fn new(key: char, description: &'static str) -> Self {
        Self { key, description }
    }
}

// ── Label ───────────────────────────────────────────────────────────────────

pub enum Label {
    Bare {
        label: Box<dyn LineRenderable>,
    },
    Hotkey {
        label: Box<dyn LineRenderable>,
        hotkey: Hotkey,
        /// When true, this is a focus-switch hotkey: highlighted when the panel
        /// is *inactive* (showing how to get there). When false, this is an
        /// action hotkey: highlighted when the panel is *active*.
        focus: bool,
    },
}

impl Label {
    pub fn bare(label: impl LineRenderable + 'static) -> Self {
        Self::Bare {
            label: Box::new(label),
        }
    }

    /// Action hotkey — highlighted when the panel is focused/active.
    pub fn hotkey(label: impl LineRenderable + 'static, hotkey: Hotkey) -> Self {
        Self::Hotkey {
            label: Box::new(label),
            hotkey,
            focus: false,
        }
    }

    /// Focus-switch hotkey — highlighted when the panel is *inactive*,
    /// showing the user how to switch focus to this panel.
    pub fn focus_hotkey(label: impl LineRenderable + 'static, hotkey: Hotkey) -> Self {
        Self::Hotkey {
            label: Box::new(label),
            hotkey,
            focus: true,
        }
    }
}

impl LineRenderable for Label {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static> {
        match self {
            Label::Bare { label } => label.render(max_width, kind),
            Label::Hotkey {
                label,
                hotkey,
                focus,
            } => {
                let inner = label.render(max_width, kind);

                // Focus labels highlight when inactive (panel not focused).
                // Action labels highlight when active (panel focused).
                let should_highlight = matches!(
                    (focus, kind),
                    (true, RenderKind::Inactive) | (false, RenderKind::Active)
                );
                if !should_highlight {
                    return inner;
                }

                // Accent the hotkey char.
                let key_lower = hotkey.key.to_ascii_lowercase();
                let mut result: Vec<Span<'static>> = Vec::new();
                let mut found = false;

                for span in inner.spans {
                    if found {
                        result.push(span);
                        continue;
                    }

                    let text: &str = span.content.as_ref();
                    let accent_pos = text
                        .char_indices()
                        .position(|(_, c)| c.to_ascii_lowercase() == key_lower);

                    match accent_pos {
                        Some(pos) => {
                            found = true;
                            let chars: Vec<char> = text.chars().collect();
                            if pos > 0 {
                                let before: String = chars[..pos].iter().collect();
                                result.push(Span::styled(before, span.style));
                            }
                            result.push(Span::styled(chars[pos].to_string(), theme::HOTKEY_ACTIVE));
                            if pos + 1 < chars.len() {
                                let after: String = chars[pos + 1..].iter().collect();
                                result.push(Span::styled(after, span.style));
                            }
                        }
                        None => result.push(span),
                    }
                }

                let mut line = if found {
                    Line::from(result)
                } else {
                    // Key not in label — prepend it.
                    let mut spans =
                        vec![Span::styled(hotkey.key.to_string(), theme::HOTKEY_ACTIVE)];
                    spans.append(&mut result);
                    Line::from(spans)
                };

                if line.width() > max_width as usize {
                    line = truncate_line(line, max_width);
                }

                line
            }
        }
    }
}

// ── String LineRenderable impl ──────────────────────────────────────────────

impl LineRenderable for String {
    fn render(&self, max_width: u16, _kind: RenderKind) -> Line<'static> {
        let line = Line::from(Span::styled(self.clone(), Style::default()));
        if self.chars().count() > max_width as usize {
            truncate_line(line, max_width)
        } else {
            line
        }
    }
}

// ── Truncation helper ───────────────────────────────────────────────────────

/// Truncate a `Line` to fit within `max_width` columns, appending `…` if truncated.
/// The `…` occupies 1 column, so content is truncated to `max_width - 1`.
pub fn truncate_line(line: Line<'static>, max_width: u16) -> Line<'static> {
    let w = max_width as usize;
    if line.width() <= w {
        return line;
    }
    if w == 0 {
        return Line::default();
    }
    let content_width = w - 1; // reserve 1 for `…`
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut remaining = content_width;

    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let char_count = span.content.chars().count();
        if char_count <= remaining {
            remaining -= char_count;
            result.push(span);
        } else {
            let truncated: String = span.content.chars().take(remaining).collect();
            result.push(Span::styled(truncated, span.style));
            remaining = 0;
        }
    }

    let ellipsis_style = result.last().map(|s| s.style).unwrap_or_default();
    result.push(Span::styled("…", ellipsis_style));

    Line::from(result)
}

// ── InputField ──────────────────────────────────────────────────────────────

pub struct InputField {
    pub label: Label,
    pub value: String,
    pub active: bool,
    pub max_input_width: u16,
}

impl InputField {
    pub fn new(label: Label, max_input_width: u16) -> Self {
        Self {
            label,
            value: String::new(),
            active: false,
            max_input_width,
        }
    }
}

impl LineRenderable for InputField {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static> {
        if !self.active {
            return self.label.render(max_width, kind);
        }

        // Active: render as "/ query_text█"
        let prefix = "/ ";
        let cursor_char = "█";

        let width = max_width.min(self.max_input_width);
        if width < 4 {
            return Line::default();
        }

        // Available space for the query text (minus prefix and cursor).
        let text_width = width as usize - prefix.len() - cursor_char.len();
        let query = if self.value.len() > text_width {
            // Show the tail of the query so the cursor stays visible.
            &self.value[self.value.len() - text_width..]
        } else {
            &self.value
        };

        Line::from(vec![
            Span::styled(prefix.to_string(), theme::INPUT_EDITING),
            Span::styled(query.to_string(), theme::INPUT_EDITING),
            Span::styled(cursor_char.to_string(), theme::INPUT_EDITING),
        ])
    }
}

impl InputField {
    /// Create an owned snapshot that implements `LineRenderable`,
    /// for use in `top_border_items()`.
    pub fn snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            active: self.active,
            // Pre-render the label in both kinds so the snapshot is self-contained.
            label_active: self.label.render(self.max_input_width, RenderKind::Active),
            label_inactive: self
                .label
                .render(self.max_input_width, RenderKind::Inactive),
            value: self.value.clone(),
            max_input_width: self.max_input_width,
        }
    }
}

/// Owned snapshot of an `InputField`'s state. Implements `LineRenderable`.
pub struct InputSnapshot {
    active: bool,
    label_active: Line<'static>,
    label_inactive: Line<'static>,
    value: String,
    max_input_width: u16,
}

impl LineRenderable for InputSnapshot {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static> {
        if !self.active {
            let line = match kind {
                RenderKind::Active => self.label_active.clone(),
                RenderKind::Inactive => self.label_inactive.clone(),
            };
            if line.width() > max_width as usize {
                return truncate_line(line, max_width);
            }
            return line;
        }

        let prefix = "/ ";
        let cursor_char = "█";

        let width = max_width.min(self.max_input_width);
        if width < 4 {
            return Line::default();
        }

        let text_width = width as usize - prefix.len() - cursor_char.len();
        let query = if self.value.len() > text_width {
            &self.value[self.value.len() - text_width..]
        } else {
            &self.value
        };

        Line::from(vec![
            Span::styled(prefix.to_string(), theme::INPUT_EDITING),
            Span::styled(query.to_string(), theme::INPUT_EDITING),
            Span::styled(cursor_char.to_string(), theme::INPUT_EDITING),
        ])
    }
}
