use std::sync::Arc;
use std::time::Duration;

use regex::RegexBuilder;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::sync::Mutex;

use super::buffer::OutputBuffer;
use crate::observe::structured::BufferEventKind;
use crate::observe::structured::StructuredLogBuilder;
use crate::observe::structured::Utf8Stream;

pub(crate) struct PtyShell {
    writer: pty_process::OwnedWritePty,
    child: Child,
    pub(crate) output_buf: OutputBuffer,
    read_task: tokio::task::JoinHandle<()>,
}

impl PtyShell {
    pub fn spawn(
        shell_command: &str,
        env: impl IntoIterator<Item = (String, String)>,
        log: StructuredLogBuilder,
        shell_name: String,
    ) -> Result<Self, pty_process::Error> {
        let (pty, pts) = pty_process::open()?;
        pty.resize(pty_process::Size::new(24, u16::MAX))?;

        let mut cmd = pty_process::Command::new(shell_command).kill_on_drop(true);
        cmd = cmd.envs(env);
        let child = cmd.spawn(pts)?;

        let (reader, writer) = pty.into_split();
        let output_buf = OutputBuffer::new();
        let output_for_reader = output_buf.clone();
        let mut reader = tokio::io::BufReader::new(reader);
        let utf8 = Arc::new(Mutex::new(Utf8Stream::new()));
        let read_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        // Emit a `Grew` buffer event with the decoded text.
                        // The streaming UTF-8 decoder holds back any partial
                        // sequence so multi-byte codepoints split across reads
                        // arrive intact.
                        let decoded = utf8.lock().await.feed(&buf[..n]);
                        if !decoded.is_empty() {
                            log.push_buffer_event(
                                &shell_name,
                                BufferEventKind::Grew { data: decoded },
                            );
                        }
                        // Append the raw bytes to the matching buffer. Matching
                        // does its own lossy decoding on the entire buffer, so
                        // raw bytes are correct here.
                        output_for_reader.append(&buf[..n]).await;
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            writer,
            child,
            output_buf,
            read_task,
        })
    }

    pub async fn init_prompt(
        &mut self,
        prompt: &str,
        timeout: Duration,
    ) -> Result<(), tokio::time::error::Elapsed> {
        let any_output_re = RegexBuilder::new(".+")
            .dot_matches_new_line(false)
            .build()
            .expect("any-output regex must be valid");

        let prompt_re = RegexBuilder::new(&format!("^{}", regex::escape(prompt)))
            .multi_line(true)
            .crlf(true)
            .build()
            .expect("prompt regex must be valid");

        tokio::time::timeout(timeout, async {
            // Step 1: Wait for any shell output (rc files, default prompt, etc.)
            loop {
                let notified = self.output_buf.notify.notified();
                if self
                    .output_buf
                    .consume_regex(&any_output_re)
                    .await
                    .is_some()
                {
                    break;
                }
                notified.await;
            }

            // Step 2: Send the prompt-setting command
            let init_cmd = format!("export PS1='{prompt}' PS2='' PROMPT_COMMAND=''\n");
            let _ = self.writer.write_all(init_cmd.as_bytes()).await;

            // Step 3: Wait for the new prompt to appear
            loop {
                let notified = self.output_buf.notify.notified();
                if self.output_buf.consume_regex(&prompt_re).await.is_some() {
                    break;
                }
                notified.await;
            }
        })
        .await?;

        Ok(())
    }

    pub async fn send_bytes(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.writer.write_all(data).await?;
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
        self.read_task.abort();
    }
}
