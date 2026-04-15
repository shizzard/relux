use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::text::Line;
use ratatui::text::Span;

use super::core::Hotkey;
use super::core::InputField;
use super::core::Label;
use super::core::hotkey_registry::HotkeyLayer;
use super::theme;
use super::traits::LineRenderable;
use super::traits::Listable;
use super::traits::MultilineRenderable;
use super::traits::RenderKind;

// ── Matchable ──────────────────────────────────────────────────────────────

pub trait Matchable {
    fn match_text(&self) -> &str;
}

impl<T: Matchable> Matchable for &T {
    fn match_text(&self) -> &str {
        (*self).match_text()
    }
}

// ── FilterMatch ────────────────────────────────────────────────────────────

struct FilterMatch {
    index: usize,
    score: i64,
    positions: Vec<usize>,
}

// ── FilterEvent ────────────────────────────────────────────────────────────

pub enum FilterEvent {
    /// Filter state changed (query changed, matches recomputed).
    Changed,
    /// User deactivated the filter (Escape / Enter / empty Backspace).
    Deactivated,
    /// Key not consumed.
    Ignored,
}

// ── FilteredView ──────────────────────────────────────────────────────────

/// Temporary wrapper yielded by `Filterable`'s iterator. Holds a reference
/// to the original item plus the fuzzy match positions for underline overlay.
pub struct FilteredView<'a, I> {
    item: I,
    positions: &'a [usize],
}

impl<I: LineRenderable> MultilineRenderable for FilteredView<'_, I> {
    fn render_lines(&self, max_width: u16) -> Vec<Line<'static>> {
        let line = self.item.render(max_width, RenderKind::Active);
        vec![underline_positions(line, self.positions)]
    }

    fn line_count(&self, _max_width: u16) -> usize {
        1
    }
}

// ── FilteredIter ──────────────────────────────────────────────────────────

pub struct FilteredIter<'a, L: Listable>
where
    for<'b> L::Item<'b>: Matchable + LineRenderable,
{
    filterable: &'a Filterable<L>,
    pos: usize,
}

impl<'a, L: Listable> Iterator for FilteredIter<'a, L>
where
    for<'b> L::Item<'b>: Matchable + LineRenderable,
{
    type Item = FilteredView<'a, L::Item<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let m = self.filterable.matches.get(self.pos)?;
        self.pos += 1;
        Some(FilteredView {
            item: self.filterable.inner.index(m.index),
            positions: &m.positions,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.filterable.matches.len() - self.pos;
        (remaining, Some(remaining))
    }
}

impl<L: Listable> ExactSizeIterator for FilteredIter<'_, L> where
    for<'b> L::Item<'b>: Matchable + LineRenderable
{
}

// ── Filterable ─────────────────────────────────────────────────────────────

pub struct Filterable<L: Listable>
where
    for<'a> L::Item<'a>: Matchable + LineRenderable,
{
    inner: L,
    input: InputField,
    matches: Vec<FilterMatch>,
}

impl<L: Listable> Filterable<L>
where
    for<'a> L::Item<'a>: Matchable + LineRenderable,
{
    pub fn new(inner: L, hotkey: Hotkey, max_input_width: u16) -> Self {
        let label = Label::hotkey("filter".to_string(), hotkey);
        Self {
            inner,
            input: InputField::new(label, max_input_width),
            matches: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.input.active
    }

    pub fn activate(&mut self) {
        self.input.active = true;
        self.input.value.clear();
        self.recompute_matches();
    }

    pub fn deactivate(&mut self) {
        self.input.active = false;
        self.input.value.clear();
        self.matches.clear();
    }

    /// Access the inner `Listable` (e.g. to replace items on reload).
    pub fn inner_mut(&mut self) -> &mut L {
        &mut self.inner
    }

    /// The hotkey layer for the panel slot. Returns `CatchAll` when active.
    pub fn hotkey_layer(&self) -> HotkeyLayer {
        if self.input.active {
            HotkeyLayer::CatchAll
        } else {
            HotkeyLayer::empty()
        }
    }

    /// Snapshot of the input field for use in `top_border_items()`.
    pub fn border_item(&self) -> Box<dyn LineRenderable> {
        Box::new(self.input.snapshot())
    }

    /// The item under the cursor in the filtered results.
    /// `cursor` comes from the wrapping `Scrollable`.
    pub fn selected(&self, cursor: usize) -> Option<L::Item<'_>> {
        self.matches.get(cursor).map(|m| self.inner.index(m.index))
    }

    /// Handle a key event while the filter is active.
    /// Does NOT handle Up/Down — cursor movement is the panel's job
    /// via `Scrollable::set_cursor()`.
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> FilterEvent {
        match event.code {
            KeyCode::Char(ch) => {
                self.input.value.push(ch);
                self.recompute_matches();
                FilterEvent::Changed
            }
            KeyCode::Backspace => {
                if self.input.value.pop().is_some() {
                    self.recompute_matches();
                    FilterEvent::Changed
                } else {
                    FilterEvent::Deactivated
                }
            }
            KeyCode::Esc | KeyCode::Enter => FilterEvent::Deactivated,
            _ => FilterEvent::Ignored,
        }
    }

    // ── Private ────────────────────────────────────────────────────────────

    fn recompute_matches(&mut self) {
        self.matches.clear();
        let query = &self.input.value;

        if query.is_empty() {
            let len = self.inner.iter().len();
            for i in 0..len {
                self.matches.push(FilterMatch {
                    index: i,
                    score: 0,
                    positions: Vec::new(),
                });
            }
            return;
        }

        for (i, item) in self.inner.iter().enumerate() {
            if let Some((score, positions)) = fuzzy_match(query, item.match_text()) {
                self.matches.push(FilterMatch {
                    index: i,
                    score,
                    positions,
                });
            }
        }

        self.matches
            .sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    }
}

// ── Listable impl ─────────────────────────────────────────────────────────

impl<L: Listable> Listable for Filterable<L>
where
    for<'a> L::Item<'a>: Matchable + LineRenderable,
{
    type Item<'a>
        = FilteredView<'a, L::Item<'a>>
    where
        Self: 'a;
    type Iter<'a>
        = FilteredIter<'a, L>
    where
        Self: 'a;

    fn iter(&self) -> Self::Iter<'_> {
        FilteredIter {
            filterable: self,
            pos: 0,
        }
    }

    fn index(&self, index: usize) -> Self::Item<'_> {
        let m = &self.matches[index];
        FilteredView {
            item: self.inner.index(m.index),
            positions: &m.positions,
        }
    }
}

// ── Fuzzy matching ─────────────────────────────────────────────────────────

fn fuzzy_match(query: &str, candidate: &str) -> Option<(i64, Vec<usize>)> {
    let query_chars: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    let candidate_chars: Vec<char> = candidate.chars().collect();

    let mut positions = Vec::with_capacity(query_chars.len());
    let mut qi = 0;

    for (ci, &cc) in candidate_chars.iter().enumerate() {
        if qi < query_chars.len() && cc.to_ascii_lowercase() == query_chars[qi] {
            positions.push(ci);
            qi += 1;
        }
    }

    if qi < query_chars.len() {
        return None;
    }

    let score = compute_score(&positions, &candidate_chars);
    Some((score, positions))
}

fn compute_score(positions: &[usize], candidate: &[char]) -> i64 {
    if positions.is_empty() {
        return 0;
    }

    let mut score: i64 = 0;

    for (i, &pos) in positions.iter().enumerate() {
        if i > 0 && pos == positions[i - 1] + 1 {
            score += 10;
        }
        if pos == 0 || matches!(candidate.get(pos - 1), Some('/' | '_' | '-' | '.')) {
            score += 8;
        }
    }

    score -= candidate.len() as i64;

    if positions.len() >= 2 {
        let span = positions[positions.len() - 1] - positions[0];
        let ideal_span = positions.len() - 1;
        score -= (span - ideal_span) as i64;
    }

    score
}

// ── Underline overlay ──────────────────────────────────────────────────────

/// Take a pre-styled `Line` and add `UNDERLINED` modifier to characters at
/// the given positions. Preserves all existing styles.
fn underline_positions(line: Line<'static>, positions: &[usize]) -> Line<'static> {
    if positions.is_empty() {
        return line;
    }

    let mut result: Vec<Span<'static>> = Vec::new();
    let mut char_offset: usize = 0;

    for span in line.spans {
        let span_chars: Vec<char> = span.content.chars().collect();
        let span_len = span_chars.len();
        let span_end = char_offset + span_len;

        // Check if any matched positions fall within this span.
        let has_matches = positions.iter().any(|&p| p >= char_offset && p < span_end);

        if !has_matches {
            result.push(span);
        } else {
            // Split the span around matched positions.
            let mut current = String::new();
            let mut current_underlined = false;

            for (i, &ch) in span_chars.iter().enumerate() {
                let global_pos = char_offset + i;
                let is_match = positions.contains(&global_pos);

                if is_match != current_underlined {
                    if !current.is_empty() {
                        let style = if current_underlined {
                            span.style.patch(theme::FILTER_MATCH)
                        } else {
                            span.style
                        };
                        result.push(Span::styled(current.clone(), style));
                        current.clear();
                    }
                    current_underlined = is_match;
                }
                current.push(ch);
            }
            if !current.is_empty() {
                let style = if current_underlined {
                    span.style.patch(theme::FILTER_MATCH)
                } else {
                    span.style
                };
                result.push(Span::styled(current, style));
            }
        }

        char_offset = span_end;
    }

    Line::from(result)
}
