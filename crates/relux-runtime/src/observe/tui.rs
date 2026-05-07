use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::observe::progress::ProgressEvent;
use crate::report::result::Failure;
use relux_core::error::DiagnosticReport;
use relux_core::table::SourceTable;

// ─── TuiEvent ───────────────────────────────────────────────

pub enum TuiEvent {
    /// A test started running in a slot.
    TestStarted {
        slot: usize,
        test_id: String,
        generation: u64,
    },
    /// Progress event for a running test.
    Progress {
        slot: usize,
        event: ProgressEvent,
        generation: u64,
    },
    /// A test finished.
    TestFinished {
        slot: usize,
        result_line: String,
        failure: Option<(Failure, Option<PathBuf>)>,
        progress_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// A test was skipped/invalid (not running, just report).
    Skipped { result_line: String },
}

pub type TuiTx = mpsc::UnboundedSender<TuiEvent>;

pub fn channel() -> (TuiTx, mpsc::UnboundedReceiver<TuiEvent>) {
    mpsc::unbounded_channel()
}

// ─── SlotState ──────────────────────────────────────────────

#[derive(Clone, Copy)]
enum TimedWait {
    Match,
    Sleep,
}

impl TimedWait {
    fn tick_char(self) -> char {
        match self {
            TimedWait::Match => '~',
            TimedWait::Sleep => 'z',
        }
    }
}

struct SlotState {
    test_id: String,
    progress: Vec<char>,
    timed_wait: Option<TimedWait>,
    generation: u64,
}

impl SlotState {
    fn new(test_id: String, generation: u64) -> Self {
        Self {
            test_id,
            progress: Vec::new(),
            timed_wait: None,
            generation,
        }
    }

    fn push(&mut self, ch: char) {
        self.progress.push(ch);
    }

    fn start_timed_wait(&mut self, kind: TimedWait) {
        self.timed_wait = Some(kind);
    }

    fn end_timed_wait(&mut self) {
        self.timed_wait = None;
    }

    /// Emit one tick character if in a timed wait. Returns true if emitted.
    fn tick(&mut self) -> bool {
        if let Some(kind) = self.timed_wait {
            self.push(kind.tick_char());
            true
        } else {
            false
        }
    }

    /// Render progress as a sliding window of `width` chars.
    fn render_progress(&self, width: usize) -> String {
        let len = self.progress.len();
        if len <= width {
            let s: String = self.progress.iter().collect();
            format!("{s:<width$}")
        } else {
            self.progress[len - width..].iter().collect()
        }
    }

    /// Collect the full progress string.
    fn collect_progress(&self) -> String {
        self.progress.iter().collect()
    }
}

// ─── Layout ─────────────────────────────────────────────────

fn layout() -> (usize, usize) {
    let width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);
    let name_width = width / 2;
    let progress_width = width - name_width - 2; // 2 for ": " separator
    (name_width, progress_width)
}

/// Truncate test_id to fit width, left-aligned, adding … prefix if needed.
fn truncate_name(name: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let name_len = name.chars().count();
    if name_len <= width {
        format!("{name:<width$}")
    } else {
        // Take the last (width - 1) chars, prefix with …
        let skip = name_len - (width - 1);
        let truncated: String = name.chars().skip(skip).collect();
        format!("\u{2026}{truncated}")
    }
}

// ─── Progress char mapping ──────────────────────────────────

fn progress_char(event: &ProgressEvent) -> Option<char> {
    match event {
        ProgressEvent::Send => Some('.'),
        ProgressEvent::MatchStart => None,
        ProgressEvent::MatchDone => Some('.'),
        ProgressEvent::SleepStart => None,
        ProgressEvent::SleepDone => None,
        ProgressEvent::ShellSwitch(_) => Some('|'),
        ProgressEvent::FnEnter(_) => Some('{'),
        ProgressEvent::FnExit => Some('}'),
        ProgressEvent::ShellSpawn => Some('s'),
        ProgressEvent::EffectSetup(_) => Some('+'),
        ProgressEvent::EffectTeardown => Some('-'),
        ProgressEvent::Cleanup => Some('c'),
        ProgressEvent::FailPattern => Some('!'),
        ProgressEvent::Timeout => Some('T'),
        ProgressEvent::Failure => Some('F'),
        ProgressEvent::Error(_) => Some('E'),
        ProgressEvent::Warning(_) => Some('W'),
        ProgressEvent::Annotation(_) => None,
    }
}

// ─── Spawn ──────────────────────────────────────────────────

pub fn spawn_tui(
    rx: mpsc::UnboundedReceiver<TuiEvent>,
    num_slots: usize,
    is_tty: bool,
    source_table: SourceTable,
    project_root: PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if is_tty {
            run_tty_renderer(rx, num_slots, &source_table, &project_root).await;
        } else {
            run_plain_renderer(rx, &source_table, &project_root).await;
        }
    })
}

// ─── Failure detail printing ────────────────────────────────

fn eprint_failure(
    failure: &Failure,
    log_dir: Option<&Path>,
    source_table: &SourceTable,
    project_root: &Path,
) {
    DiagnosticReport::from(failure).eprint(source_table, Some(project_root));
    if let Some(log_dir) = log_dir {
        eprintln!(
            "  Event log: file://{}",
            log_dir.join("events.json").display()
        );
    }
}

// ─── Plain (non-TTY) renderer ───────────────────────────────

async fn run_plain_renderer(
    mut rx: mpsc::UnboundedReceiver<TuiEvent>,
    source_table: &SourceTable,
    project_root: &Path,
) {
    while let Some(event) = rx.recv().await {
        match event {
            TuiEvent::TestStarted { .. } | TuiEvent::Progress { .. } => {}
            TuiEvent::TestFinished {
                slot: _,
                result_line,
                failure,
                progress_tx,
            } => {
                eprintln!("{result_line}");
                if let Some((f, log_dir)) = &failure {
                    eprint_failure(f, log_dir.as_deref(), source_table, project_root);
                }
                let _ = progress_tx.send(String::new());
            }
            TuiEvent::Skipped { result_line } => {
                eprintln!("{result_line}");
            }
        }
    }
}

// ─── TTY renderer ───────────────────────────────────────────

fn has_timed_waits(slots: &[Option<SlotState>]) -> bool {
    slots
        .iter()
        .any(|s| s.as_ref().is_some_and(|s| s.timed_wait.is_some()))
}

async fn run_tty_renderer(
    mut rx: mpsc::UnboundedReceiver<TuiEvent>,
    num_slots: usize,
    source_table: &SourceTable,
    project_root: &Path,
) {
    let mut slots: Vec<Option<SlotState>> = (0..num_slots).map(|_| None).collect();
    let mut active_lines: usize = 0;
    let tick_interval = std::time::Duration::from_secs(1);

    loop {
        let event = if has_timed_waits(&slots) {
            match tokio::time::timeout(tick_interval, rx.recv()).await {
                Ok(Some(ev)) => Some(ev),
                Ok(None) => break, // channel closed
                Err(_) => {
                    // Tick: emit one timed wait character per waiting slot
                    let mut any_ticked = false;
                    for state in slots.iter_mut().flatten() {
                        if state.tick() {
                            any_ticked = true;
                        }
                    }
                    if any_ticked {
                        let (name_width, progress_width) = layout();
                        active_lines =
                            redraw_active(&slots, active_lines, name_width, progress_width);
                    }
                    continue;
                }
            }
        } else {
            match rx.recv().await {
                Some(ev) => Some(ev),
                None => break,
            }
        };

        let Some(event) = event else { break };
        let (name_width, progress_width) = layout();

        match event {
            TuiEvent::TestStarted {
                slot,
                test_id,
                generation,
            } => {
                slots[slot] = Some(SlotState::new(test_id, generation));
                active_lines = redraw_active(&slots, active_lines, name_width, progress_width);
            }
            TuiEvent::Progress {
                slot,
                event,
                generation,
            } => {
                if let Some(state) = &mut slots[slot] {
                    if state.generation != generation {
                        continue;
                    }
                    // Track timed waits
                    match &event {
                        ProgressEvent::MatchStart => state.start_timed_wait(TimedWait::Match),
                        ProgressEvent::MatchDone => state.end_timed_wait(),
                        ProgressEvent::SleepStart => state.start_timed_wait(TimedWait::Sleep),
                        ProgressEvent::SleepDone => state.end_timed_wait(),
                        ProgressEvent::FailPattern | ProgressEvent::Timeout => {
                            state.end_timed_wait()
                        }
                        _ => {}
                    }
                    if let Some(ch) = progress_char(&event) {
                        state.push(ch);
                    }
                }
                active_lines = redraw_active(&slots, active_lines, name_width, progress_width);
            }
            TuiEvent::TestFinished {
                slot,
                result_line,
                failure,
                progress_tx,
            } => {
                let progress_string = slots[slot]
                    .as_ref()
                    .map(|s| s.collect_progress())
                    .unwrap_or_default();
                let _ = progress_tx.send(progress_string);

                slots[slot] = None;
                clear_active(active_lines);
                eprintln!("{result_line}");
                if let Some((f, log_dir)) = &failure {
                    eprint_failure(f, log_dir.as_deref(), source_table, project_root);
                }
                active_lines = redraw_active(&slots, 0, name_width, progress_width);
            }
            TuiEvent::Skipped { result_line } => {
                clear_active(active_lines);
                eprintln!("{result_line}");
                active_lines = redraw_active(&slots, 0, name_width, progress_width);
            }
        }
    }

    // Final cleanup: clear any remaining active lines
    clear_active(active_lines);
}

fn clear_active(lines: usize) {
    if lines == 0 {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    // Move up and clear each line
    for _ in 0..lines {
        write!(stderr, "\x1b[A\x1b[2K").ok();
    }
    write!(stderr, "\r").ok();
    stderr.flush().ok();
}

fn redraw_active(
    slots: &[Option<SlotState>],
    prev_active_lines: usize,
    name_width: usize,
    progress_width: usize,
) -> usize {
    let mut stderr = std::io::stderr().lock();

    // Move cursor up to overwrite previous active area
    if prev_active_lines > 0 {
        for _ in 0..prev_active_lines {
            write!(stderr, "\x1b[A").ok();
        }
        write!(stderr, "\r").ok();
    }

    let mut lines_written = 0;
    for state in slots.iter().flatten() {
        let name = truncate_name(&state.test_id, name_width);
        let progress = state.render_progress(progress_width);
        writeln!(stderr, "\x1b[2K{name}: {progress}").ok();
        lines_written += 1;
    }

    // Clear any leftover lines from previous draw
    for _ in lines_written..prev_active_lines {
        writeln!(stderr, "\x1b[2K").ok();
    }
    // Move cursor back up for cleared leftover lines
    let leftover = prev_active_lines.saturating_sub(lines_written);
    if leftover > 0 {
        for _ in 0..leftover {
            write!(stderr, "\x1b[A").ok();
        }
    }

    stderr.flush().ok();
    lines_written
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_name() {
        assert_eq!(truncate_name("foo", 10), "foo       ");
    }

    #[test]
    fn truncate_exact_name() {
        assert_eq!(truncate_name("1234567890", 10), "1234567890");
    }

    #[test]
    fn truncate_long_name() {
        let result = truncate_name("very/long/test/path/name", 10);
        assert_eq!(result, "\u{2026}path/name");
        assert_eq!(result.chars().count(), 10);
    }

    #[test]
    fn truncate_zero_width() {
        assert_eq!(truncate_name("anything", 0), "");
    }

    #[test]
    fn progress_within_window() {
        let mut s = SlotState::new("t".into(), 1);
        s.push('.');
        s.push('.');
        assert_eq!(s.render_progress(5), "..   ");
    }

    #[test]
    fn progress_exact_window() {
        let mut s = SlotState::new("t".into(), 1);
        for _ in 0..5 {
            s.push('.');
        }
        assert_eq!(s.render_progress(5), ".....");
    }

    #[test]
    fn progress_sliding_window() {
        let mut s = SlotState::new("t".into(), 1);
        for _ in 0..10 {
            s.push('.');
        }
        s.push('!');
        let rendered = s.render_progress(5);
        assert_eq!(rendered, "....!");
    }

    #[test]
    fn progress_empty() {
        let s = SlotState::new("t".into(), 1);
        assert_eq!(s.render_progress(5), "     ");
    }

    #[test]
    fn collect_progress_full() {
        let mut s = SlotState::new("t".into(), 1);
        s.push('.');
        s.push('{');
        s.push('}');
        assert_eq!(s.collect_progress(), ".{}");
    }

    #[test]
    fn progress_char_mapping() {
        assert_eq!(progress_char(&ProgressEvent::Send), Some('.'));
        assert_eq!(progress_char(&ProgressEvent::MatchStart), None);
        assert_eq!(progress_char(&ProgressEvent::MatchDone), Some('.'));
        assert_eq!(progress_char(&ProgressEvent::ShellSpawn), Some('s'));
        assert_eq!(progress_char(&ProgressEvent::Failure), Some('F'));
    }
}
