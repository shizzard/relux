use std::sync::Arc;
use std::time::Duration;

use regex::RegexBuilder;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::sync::Mutex;

use super::buffer::OutputBuffer;
use crate::runtime::observe::shell_log::ShellLogger;

pub(crate) struct PtyShell {
    writer: pty_process::OwnedWritePty,
    child: Child,
    pub(crate) output_buf: OutputBuffer,
    read_task: tokio::task::JoinHandle<()>,
    shell_log: Arc<Mutex<ShellLogger>>,
}

impl PtyShell {
    pub fn spawn(
        shell_command: &str,
        env: impl IntoIterator<Item = (String, String)>,
        shell_log: Arc<Mutex<ShellLogger>>,
    ) -> Result<Self, pty_process::Error> {
        let (pty, pts) = pty_process::open()?;

        let mut cmd = pty_process::Command::new(shell_command).kill_on_drop(true);
        cmd = cmd.envs(env);
        let child = cmd.spawn(pts)?;

        let (reader, writer) = pty.into_split();
        let output_buf = OutputBuffer::new();
        let output_for_reader = output_buf.clone();
        let shell_log_reader = shell_log.clone();
        let mut reader = tokio::io::BufReader::new(reader);
        let read_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        shell_log_reader.lock().await.log_stdout(&buf[..n]);
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
            shell_log,
        })
    }

    pub async fn init_prompt(
        &mut self,
        prompt: &str,
        timeout: Duration,
    ) -> Result<(), tokio::time::error::Elapsed> {
        let init_cmd = format!("export PS1='{prompt}' PS2='' PROMPT_COMMAND=''\n");
        let _ = self.writer.write_all(init_cmd.as_bytes()).await;

        let prompt_re = RegexBuilder::new(&format!("^{}", regex::escape(prompt)))
            .multi_line(true)
            .crlf(true)
            .build()
            .expect("prompt regex must be valid");

        tokio::time::timeout(timeout, async {
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
        self.shell_log.lock().await.log_stdin(data);
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        let _ = self.child.kill().await;
        self.read_task.abort();
    }
}
