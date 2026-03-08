use std::io::Write;
use std::time::Instant;

use colored::Colorize;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Send,
    MatchStart,
    MatchDone,
    SleepStart,
    SleepDone,
    ShellSwitch(String),
    FnEnter(String),
    FnExit,
    EffectSetup(String),
    Cleanup,
    FailPattern,
    Timeout,
    Failure,
    Error(String),
    Warning(String),
    Annotation(String),
}

pub type ProgressTx = mpsc::UnboundedSender<ProgressEvent>;

pub fn channel() -> (ProgressTx, mpsc::UnboundedReceiver<ProgressEvent>) {
    mpsc::unbounded_channel()
}

enum TimedWait {
    Match,
    Sleep,
}

/// Spawns the progress printer task. Returns a JoinHandle that resolves
/// to the collected progress string once all senders are dropped.
pub fn spawn_printer(
    mut rx: mpsc::UnboundedReceiver<ProgressEvent>,
) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let mut collected = String::new();
        let mut timed: Option<(TimedWait, Instant)> = None;
        let mut timed_tick_count: usize = 0;

        loop {
            let event = if timed.is_some() {
                match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
                    Ok(Some(ev)) => Some(ev),
                    Ok(None) => None,
                    Err(_) => {
                        if let Some((kind, started)) = &timed {
                            let ch = match kind {
                                TimedWait::Match => '~',
                                TimedWait::Sleep => 'z',
                            };
                            let elapsed_secs = started.elapsed().as_secs() as usize;
                            while timed_tick_count < elapsed_secs {
                                emit(&mut collected, ch);
                                timed_tick_count += 1;
                            }
                        }
                        continue;
                    }
                }
            } else {
                rx.recv().await
            };

            let Some(event) = event else {
                break;
            };

            match event {
                ProgressEvent::Send => {
                    emit(&mut collected, '.');
                }
                ProgressEvent::MatchStart => {
                    timed = Some((TimedWait::Match, Instant::now()));
                    timed_tick_count = 0;
                }
                ProgressEvent::MatchDone => {
                    timed = None;
                    emit(&mut collected, '.');
                }
                ProgressEvent::SleepStart => {
                    timed = Some((TimedWait::Sleep, Instant::now()));
                    timed_tick_count = 0;
                }
                ProgressEvent::SleepDone => {
                    timed = None;
                }
                ProgressEvent::ShellSwitch(_) => {
                    emit(&mut collected, '|');
                }
                ProgressEvent::FnEnter(_) => {
                    emit(&mut collected, '{');
                }
                ProgressEvent::FnExit => {
                    emit(&mut collected, '}');
                }
                ProgressEvent::EffectSetup(_) => {
                    emit(&mut collected, '+');
                }
                ProgressEvent::Cleanup => {
                    emit(&mut collected, 'c');
                }
                ProgressEvent::FailPattern => {
                    timed = None;
                    emit(&mut collected, '!');
                }
                ProgressEvent::Timeout => {
                    timed = None;
                    emit(&mut collected, 'T');
                }
                ProgressEvent::Failure => {
                    timed = None;
                    emit(&mut collected, 'F');
                }
                ProgressEvent::Error(_) => {
                    timed = None;
                    emit(&mut collected, 'E');
                }
                ProgressEvent::Warning(_) => {
                    emit(&mut collected, 'W');
                }
                ProgressEvent::Annotation(text) => {
                    let s = format!("({text})");
                    collected.push_str(&s);
                    eprint!("{}", s.dimmed());
                    let _ = std::io::stderr().flush();
                }
            }
        }

        collected
    })
}

fn emit(collected: &mut String, ch: char) {
    collected.push(ch);
    let s = ch.to_string().dimmed();
    eprint!("{s}");
    let _ = std::io::stderr().flush();
}
