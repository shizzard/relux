use std::io::Write;
use std::time::Instant;

use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Send,
    MatchStart,
    MatchDone,
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
}

pub type ProgressTx = mpsc::UnboundedSender<ProgressEvent>;

pub fn channel() -> (ProgressTx, mpsc::UnboundedReceiver<ProgressEvent>) {
    mpsc::unbounded_channel()
}

/// Spawns the progress printer task. Returns a JoinHandle that resolves
/// to the collected progress string once all senders are dropped.
pub fn spawn_printer(
    mut rx: mpsc::UnboundedReceiver<ProgressEvent>,
) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let mut collected = String::new();
        let mut match_start: Option<Instant> = None;
        let mut last_tilde_count: usize = 0;

        loop {
            let event = if match_start.is_some() {
                // While awaiting a match, use a 1-second timeout to emit tildes
                match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
                    Ok(Some(ev)) => Some(ev),
                    Ok(None) => None, // channel closed
                    Err(_) => {
                        // Timeout elapsed -- emit a tilde for the waiting match
                        if let Some(started) = match_start {
                            let elapsed_secs = started.elapsed().as_secs() as usize;
                            while last_tilde_count < elapsed_secs {
                                emit(&mut collected, '~');
                                last_tilde_count += 1;
                            }
                        }
                        continue;
                    }
                }
            } else {
                rx.recv().await
            };

            let Some(event) = event else {
                break; // channel closed
            };

            match event {
                ProgressEvent::Send => {
                    emit(&mut collected, '.');
                }
                ProgressEvent::MatchStart => {
                    match_start = Some(Instant::now());
                    last_tilde_count = 0;
                }
                ProgressEvent::MatchDone => {
                    match_start = None;
                    emit(&mut collected, '.');
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
                    match_start = None;
                    emit(&mut collected, '!');
                }
                ProgressEvent::Timeout => {
                    match_start = None;
                    emit(&mut collected, 'T');
                }
                ProgressEvent::Failure => {
                    match_start = None;
                    emit(&mut collected, 'F');
                }
                ProgressEvent::Error(_) => {
                    match_start = None;
                    emit(&mut collected, 'E');
                }
                ProgressEvent::Warning(_) => {
                    emit(&mut collected, 'W');
                }
            }
        }

        eprintln!();
        collected
    })
}

fn emit(collected: &mut String, ch: char) {
    collected.push(ch);
    eprint!("{ch}");
    let _ = std::io::stderr().flush();
}
