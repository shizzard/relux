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
    },
}

impl Label {
    pub fn bare(label: impl LineRenderable + 'static) -> Self {
        Self::Bare {
            label: Box::new(label),
        }
    }

    pub fn hotkey(label: impl LineRenderable + 'static, hotkey: Hotkey) -> Self {
        Self::Hotkey {
            label: Box::new(label),
            hotkey,
        }
    }
}

impl LineRenderable for Label {
    fn render(&self, max_width: u16, kind: RenderKind) -> Line<'static> {
        match self {
            Label::Bare { label } => label.render(max_width, kind),
            Label::Hotkey { label, hotkey } => {
                let inner = label.render(max_width, kind);

                if kind == RenderKind::Active {
                    return inner;
                }

                // Inactive: accent the hotkey char so user knows how to focus.
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
                            result.push(Span::styled(
                                chars[pos].to_string(),
                                theme::HOTKEY_ACTIVE,
                            ));
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
                    let mut spans = vec![Span::styled(
                        hotkey.key.to_string(),
                        theme::HOTKEY_ACTIVE,
                    )];
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

// ── Blanket impl for strings ────────────────────────────────────────────────

impl<T: AsRef<str>> LineRenderable for T {
    fn render(&self, max_width: u16, _kind: RenderKind) -> Line<'static> {
        let text = self.as_ref();
        let line = Line::from(Span::styled(text.to_owned(), Style::default()));
        if text.chars().count() > max_width as usize {
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
}
