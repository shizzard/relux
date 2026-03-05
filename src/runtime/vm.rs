use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{Mutex, Notify, watch};

use crate::dsl::resolver::ir::{self, Expr, ShellStmt, Span, Spanned};
use crate::runtime::result::Failure;
use crate::runtime::vars::{VariableStack, interpolate};
use crate::runtime::{CodeServer, DEFAULT_SHELL_PROMPT};

fn regex_error_summary(e: &regex::Error) -> String {
    let full = e.to_string();
    full.lines()
        .rev()
        .find(|l| !l.is_empty())
        .unwrap_or(&full)
        .strip_prefix("error: ")
        .unwrap_or(&full)
        .to_string()
}

#[derive(Clone, Debug)]
pub enum FailPattern {
    Regex(Regex),
    Literal(String),
}

#[derive(Clone, Debug, Default)]
pub struct OutputBuffer {
    data: Arc<Mutex<Vec<u8>>>,
    notify: Arc<Notify>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(Vec::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn append(&self, bytes: &[u8]) {
        self.data.lock().await.extend_from_slice(bytes);
        self.notify.notify_waiters();
    }

    pub async fn snapshot(&self) -> Vec<u8> {
        self.data.lock().await.clone()
    }

    pub async fn find_literal_from(&self, cursor: usize, needle: &str) -> Option<(usize, usize)> {
        let hay = self.data.lock().await;
        let slice = &hay[cursor.min(hay.len())..];
        let hay_text = String::from_utf8_lossy(slice);
        let pos_chars = hay_text.find(needle)?;
        let pre = &hay_text[..pos_chars];
        let start = cursor + pre.len();
        let end = start + needle.len();
        Some((start, end))
    }

    pub async fn find_regex_from(
        &self,
        cursor: usize,
        re: &Regex,
    ) -> Option<(usize, usize, HashMap<String, String>)> {
        let hay = self.data.lock().await;
        let slice = &hay[cursor.min(hay.len())..];
        let hay_text = String::from_utf8_lossy(slice);
        let cap = re.captures(&hay_text)?;
        let whole = cap.get(0)?;
        let pre = &hay_text[..whole.start()];
        let start = cursor + pre.len();
        let end = start + whole.as_str().len();

        let mut captures = HashMap::new();
        for i in 0..cap.len() {
            if let Some(m) = cap.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }
        Some((start, end, captures))
    }
}

pub struct Vm {
    writer: pty_process::OwnedWritePty,
    child: Child,
    output_buf: OutputBuffer,
    read_task: tokio::task::JoinHandle<()>,
    fail_watcher_tx: watch::Sender<Option<FailPattern>>,
    fail_triggered: Arc<AtomicBool>,
    fail_detail: Arc<Mutex<Option<(String, String)>>>,
    vars: VariableStack,
    timeout: Duration,
    code_server: Arc<CodeServer>,
    shell_name: String,
    cursor: usize,
}

impl Vm {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        shell_name: String,
        vars: VariableStack,
        timeout: Duration,
        code_server: Arc<CodeServer>,
    ) -> Result<Self, Failure> {
        let (pty, pts) = pty_process::open().map_err(|e| Failure::Runtime {
            message: format!("failed to allocate pty: {e}"),
            span: None,
            shell: Some(shell_name.clone()),
        })?;

        let mut cmd = pty_process::Command::new("/bin/sh").kill_on_drop(true);
        cmd = cmd.envs(vars.process_env());
        let child = cmd.spawn(pts).map_err(|e| Failure::Runtime {
            message: format!("failed to spawn shell: {e}"),
            span: None,
            shell: Some(shell_name.clone()),
        })?;

        let (reader, writer) = pty.into_split();
        let output_buf = OutputBuffer::new();
        let output_for_reader = output_buf.clone();
        let mut reader = tokio::io::BufReader::new(reader);
        let read_task = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => output_for_reader.append(&buf[..n]).await,
                    Err(_) => break,
                }
            }
        });

        let (fail_watcher_tx, fail_watcher_rx) = watch::channel(None);
        let fail_triggered = Arc::new(AtomicBool::new(false));
        let fail_detail = Arc::new(Mutex::new(None));
        spawn_fail_watcher(
            output_buf.clone(),
            fail_watcher_rx,
            fail_triggered.clone(),
            fail_detail.clone(),
        );

        let mut vm = Self {
            writer,
            child,
            output_buf,
            read_task,
            fail_watcher_tx,
            fail_triggered,
            fail_detail,
            vars,
            timeout,
            code_server,
            shell_name: shell_name.clone(),
            cursor: 0,
        };

        vm.init_prompt().await.map_err(|_| Failure::Runtime {
            message: "shell did not produce prompt during init".to_string(),
            span: None,
            shell: Some(shell_name),
        })?;

        Ok(vm)
    }

    async fn init_prompt(&mut self) -> Result<(), tokio::time::error::Elapsed> {
        let init_cmd = format!(
            "export PS1='{prompt}' PS2='' PROMPT_COMMAND=''\n",
            prompt = DEFAULT_SHELL_PROMPT,
        );
        let _ = self.writer.write_all(init_cmd.as_bytes()).await;

        tokio::time::timeout(self.timeout, async {
            loop {
                if self
                    .output_buf
                    .find_literal_from(self.cursor, DEFAULT_SHELL_PROMPT)
                    .await
                    .is_some()
                {
                    break;
                }
                self.output_buf.notify.notified().await;
            }
        })
        .await?;

        let snapshot = self.output_buf.snapshot().await;
        self.cursor = snapshot.len();
        Ok(())
    }

    pub async fn exec_stmts(&mut self, stmts: &[Spanned<ShellStmt>]) -> Result<String, Failure> {
        let mut last = String::new();
        for stmt in stmts {
            last = self.exec_stmt(stmt).await?;
        }
        Ok(last)
    }

    pub async fn exec_stmt(&mut self, stmt: &Spanned<ShellStmt>) -> Result<String, Failure> {
        self.check_fail(stmt.span.clone()).await?;
        match &stmt.node {
            ShellStmt::FailRegex(expr) => {
                let pat = interpolate(expr, &self.vars).await;
                let re = Regex::new(&pat).map_err(|e| Failure::Runtime {
                    message: format!("invalid fail regex: {}", regex_error_summary(&e)),
                    span: Some(expr.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                let _ = self.fail_watcher_tx.send(Some(FailPattern::Regex(re)));
                Ok(String::new())
            }
            ShellStmt::FailLiteral(expr) => {
                let pat = interpolate(expr, &self.vars).await;
                let _ = self.fail_watcher_tx.send(Some(FailPattern::Literal(pat)));
                Ok(String::new())
            }
            ShellStmt::Timeout(d) => {
                self.timeout = *d;
                Ok(String::new())
            }
            ShellStmt::Let(decl) => {
                let value = if let Some(expr) = &decl.value {
                    self.eval_expr(expr).await?
                } else {
                    String::new()
                };
                self.vars.let_insert(decl.name.node.clone(), value.clone());
                Ok(value)
            }
            ShellStmt::Assign(assign) => {
                let value = self.eval_expr(&assign.value).await?;
                let _ = self.vars.assign(&assign.name.node, value.clone()).await;
                Ok(value)
            }
            ShellStmt::Expr(expr) => {
                self.eval_expr(&Spanned::new(expr.clone(), stmt.span.clone()))
                    .await
            }
        }
    }

    #[async_recursion::async_recursion]
    async fn eval_expr(&mut self, expr: &Spanned<Expr>) -> Result<String, Failure> {
        self.check_fail(expr.span.clone()).await?;
        match &expr.node {
            Expr::String(s) => Ok(interpolate(s, &self.vars).await),
            Expr::Var(name) => Ok(self.vars.lookup(name).await.unwrap_or_default()),
            Expr::Send(s) => {
                let payload = interpolate(s, &self.vars).await;
                self.send_bytes(format!("{payload}\n").as_bytes(), expr.span.clone())
                    .await?;
                Ok(payload)
            }
            Expr::SendRaw(s) => {
                let payload = interpolate(s, &self.vars).await;
                self.send_bytes(payload.as_bytes(), expr.span.clone())
                    .await?;
                Ok(payload)
            }
            Expr::MatchLiteral(s) => {
                let pattern = interpolate(s, &self.vars).await;
                let (start, end) = self.wait_for_literal(&pattern, expr.span.clone()).await?;
                self.cursor = end.max(start);
                Ok(pattern)
            }
            Expr::MatchRegex(s) => {
                let pattern = interpolate(s, &self.vars).await;
                let re = Regex::new(&pattern).map_err(|e| Failure::Runtime {
                    message: format!("invalid regex: {}", regex_error_summary(&e)),
                    span: Some(s.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                let (_start, end, captures) = self
                    .wait_for_regex(&pattern, &re, expr.span.clone())
                    .await?;
                self.cursor = end;
                let full = captures.get("0").cloned().unwrap_or_default();
                self.vars.set_captures(captures);
                Ok(full)
            }
            Expr::Call(call) => self.eval_call(call, &expr.span).await,
        }
    }

    async fn eval_call(&mut self, call: &ir::FnCall, span: &Span) -> Result<String, Failure> {
        let fn_id = self
            .code_server
            .lookup(&call.name.node, call.args.len())
            .ok_or_else(|| Failure::Runtime {
                message: format!(
                    "undefined function `{}` with arity {}",
                    call.name.node,
                    call.args.len()
                ),
                span: Some(span.clone()),
                shell: Some(self.shell_name.clone()),
            })?;
        let (params, body) = {
            let func = self
                .code_server
                .get(fn_id)
                .ok_or_else(|| Failure::Runtime {
                    message: format!("invalid function id {fn_id}"),
                    span: Some(span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
            (func.params.clone(), func.body.clone())
        };

        let mut evaluated_args = Vec::with_capacity(call.args.len());
        for arg in &call.args {
            evaluated_args.push(self.eval_expr(arg).await?);
        }

        self.vars.push_frame();
        for (param, value) in params.iter().zip(evaluated_args.into_iter()) {
            self.vars.let_insert(param.node.clone(), value);
        }
        let mut last = String::new();
        for stmt in &body {
            last = self.exec_stmt(stmt).await?;
        }
        self.vars.pop_frame();
        Ok(last)
    }

    async fn wait_for_literal(&self, pattern: &str, span: Span) -> Result<(usize, usize), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some(r) = self
                    .output_buf
                    .find_literal_from(self.cursor, pattern)
                    .await
                {
                    return Ok::<(usize, usize), Failure>(r);
                }
                self.output_buf.notify.notified().await;
            }
        };

        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| Failure::MatchTimeout {
                pattern: pattern.to_string(),
                span,
                shell: self.shell_name.clone(),
            })?
    }

    async fn wait_for_regex(
        &self,
        pattern: &str,
        re: &Regex,
        span: Span,
    ) -> Result<(usize, usize, HashMap<String, String>), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some(r) = self.output_buf.find_regex_from(self.cursor, re).await {
                    return Ok::<(usize, usize, HashMap<String, String>), Failure>(r);
                }
                self.output_buf.notify.notified().await;
            }
        };

        tokio::time::timeout(self.timeout, fut)
            .await
            .map_err(|_| Failure::MatchTimeout {
                pattern: pattern.to_string(),
                span,
                shell: self.shell_name.clone(),
            })?
    }

    async fn check_fail(&self, span: Span) -> Result<(), Failure> {
        if self.fail_triggered.load(Ordering::Relaxed) {
            let detail = self.fail_detail.lock().await.clone().unwrap_or_default();
            return Err(Failure::FailPatternMatched {
                pattern: detail.0,
                matched_line: detail.1,
                span,
                shell: self.shell_name.clone(),
            });
        }
        Ok(())
    }

    async fn send_bytes(&mut self, data: &[u8], span: Span) -> Result<(), Failure> {
        self.writer
            .write_all(data)
            .await
            .map_err(|e| Failure::ShellExited {
                shell: self.shell_name.clone(),
                exit_code: e.raw_os_error(),
                span,
            })
    }

    pub async fn output_snapshot(&self) -> Vec<u8> {
        self.output_buf.snapshot().await
    }

    pub async fn reset_for_reuse(&mut self, timeout: Duration) {
        self.timeout = timeout;
        self.cursor = 0;
        self.fail_triggered.store(false, Ordering::Relaxed);
        *self.fail_detail.lock().await = None;
        let _ = self.fail_watcher_tx.send(None);
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        self.read_task.abort();
    }
}

fn spawn_fail_watcher(
    output: OutputBuffer,
    mut rx: watch::Receiver<Option<FailPattern>>,
    flag: Arc<AtomicBool>,
    detail: Arc<Mutex<Option<(String, String)>>>,
) {
    tokio::spawn(async move {
        let mut cursor = 0usize;
        loop {
            let current = rx.borrow().clone();
            if let Some(pattern) = current {
                loop {
                    let data = output.snapshot().await;
                    let slice = &data[cursor.min(data.len())..];
                    let text = String::from_utf8_lossy(slice);
                    let matched = match &pattern {
                        FailPattern::Regex(re) => re
                            .find(&text)
                            .map(|m| (re.as_str().to_string(), m.as_str().to_string(), m.end())),
                        FailPattern::Literal(s) => text.find(s).map(|start| {
                            let end = start + s.len();
                            (s.clone(), s.clone(), end)
                        }),
                    };
                    if let Some((p, line, end)) = matched {
                        flag.store(true, Ordering::Relaxed);
                        *detail.lock().await = Some((p, line));
                        cursor += end;
                        break;
                    }

                    tokio::select! {
                        _ = output.notify.notified() => {},
                        changed = rx.changed() => {
                            if changed.is_err() {
                                return;
                            }
                            break;
                        }
                    }
                }
            } else if rx.changed().await.is_err() {
                return;
            }
        }
    });
}
