use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{Mutex, Notify, watch};

use crate::dsl::resolver::ir::{self, Expr, ShellStmt, Span, Spanned};
use crate::runtime::event_log::{BufferSnapshot, EventCollector, LogEventKind};
use crate::runtime::result::Failure;
use crate::runtime::shell_log::ShellLogger;
use crate::runtime::vars::{FailPattern, ScopeStack, interpolate};
use crate::runtime::bifs::{PureContext, VmContext};
use crate::runtime::progress::{ProgressEvent, ProgressTx};
use crate::runtime::{Callable, CodeServer};

const BUFFER_PREFIX_LEN: usize = 40;
const BUFFER_SUFFIX_LEN: usize = 40;
const BUFFER_TAIL_LEN: usize = 80;

fn truncate_before(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let start = s.ceil_char_boundary(s.len() - max);
        format!("...{}", &s[start..])
    }
}

fn truncate_after(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max);
        format!("{}...", &s[..end])
    }
}

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
    scope: ScopeStack,
    code_server: Arc<CodeServer>,
    shell_name: String,
    shell_prompt: String,
    cursor: usize,
    progress_tx: Option<ProgressTx>,
    shell_log: Arc<Mutex<ShellLogger>>,
    event_collector: Option<EventCollector>,
}

impl Vm {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        shell_name: String,
        shell_prompt: String,
        shell_command: String,
        scope: ScopeStack,
        code_server: Arc<CodeServer>,
        progress_tx: Option<ProgressTx>,
        log_dir: &Path,
        test_start: Instant,
        event_collector: Option<EventCollector>,
    ) -> Result<Self, Failure> {
        let shell_log = ShellLogger::create(log_dir, &shell_name, test_start).map_err(|e| {
            Failure::Runtime {
                message: format!("failed to create shell log: {e}"),
                span: None,
                shell: Some(shell_name.clone()),
            }
        })?;
        let shell_log = Arc::new(Mutex::new(shell_log));

        let (pty, pts) = pty_process::open().map_err(|e| Failure::Runtime {
            message: format!("failed to allocate pty: {e}"),
            span: None,
            shell: Some(shell_name.clone()),
        })?;

        let mut cmd = pty_process::Command::new(&shell_command).kill_on_drop(true);
        cmd = cmd.envs(scope.process_env());
        let child = cmd.spawn(pts).map_err(|e| Failure::Runtime {
            message: format!("failed to spawn shell: {e}"),
            span: None,
            shell: Some(shell_name.clone()),
        })?;

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
            scope,
            code_server,
            shell_name: shell_name.clone(),
            shell_prompt,
            cursor: 0,
            progress_tx,
            shell_log,
            event_collector,
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
            prompt = self.shell_prompt,
        );
        let _ = self.writer.write_all(init_cmd.as_bytes()).await;

        let prompt_re = RegexBuilder::new(&format!("^{}", regex::escape(&self.shell_prompt)))
            .multi_line(true)
            .crlf(true)
            .build()
            .expect("prompt regex must be valid");

        tokio::time::timeout(self.scope.timeout(), async {
            loop {
                if let Some((_start, end, _captures)) = self
                    .output_buf
                    .find_regex_from(self.cursor, &prompt_re)
                    .await
                {
                    self.cursor = end;
                    break;
                }
                self.output_buf.notify.notified().await;
            }
        })
        .await?;

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
                let pat = interpolate(expr, &self.scope).await;
                self.emit_event(LogEventKind::FailPatternSet { pattern: pat.clone() }).await;
                let re = RegexBuilder::new(&pat).multi_line(true).crlf(true).build().map_err(|e| Failure::Runtime {
                    message: format!("invalid fail regex: {}", regex_error_summary(&e)),
                    span: Some(expr.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                let pattern = Some(FailPattern::Regex(re));
                self.scope.set_fail_pattern(pattern.clone());
                let _ = self.fail_watcher_tx.send(pattern);
                Ok(String::new())
            }
            ShellStmt::FailLiteral(expr) => {
                let pat = interpolate(expr, &self.scope).await;
                self.emit_event(LogEventKind::FailPatternSet { pattern: pat.clone() }).await;
                let pattern = Some(FailPattern::Literal(pat));
                self.scope.set_fail_pattern(pattern.clone());
                let _ = self.fail_watcher_tx.send(pattern);
                Ok(String::new())
            }
            ShellStmt::ClearFailPattern => {
                self.emit_event(LogEventKind::FailPatternCleared).await;
                self.scope.set_fail_pattern(None);
                let _ = self.fail_watcher_tx.send(None);
                Ok(String::new())
            }
            ShellStmt::Timeout(d) => {
                self.scope.set_timeout(*d);
                Ok(String::new())
            }
            ShellStmt::Let(decl) => {
                let value = if let Some(expr) = &decl.value {
                    self.eval_expr(expr).await?
                } else {
                    String::new()
                };
                self.emit_event(LogEventKind::VarLet {
                    name: decl.name.node.clone(),
                    value: value.clone(),
                }).await;
                self.scope.let_insert(decl.name.node.clone(), value.clone());
                Ok(value)
            }
            ShellStmt::Assign(assign) => {
                let value = self.eval_expr(&assign.value).await?;
                let found = self.scope.assign(&assign.name.node, value.clone()).await;
                if !found {
                    return Err(Failure::Runtime {
                        message: format!(
                            "assignment to undeclared variable `{}`",
                            assign.name.node
                        ),
                        span: Some(assign.name.span.clone()),
                        shell: Some(self.shell_name.clone()),
                    });
                }
                self.emit_event(LogEventKind::VarAssign {
                    name: assign.name.node.clone(),
                    value: value.clone(),
                }).await;
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
            Expr::String(s) => Ok(interpolate(s, &self.scope).await),
            Expr::Var(name) => Ok(self.scope.lookup(name).await.unwrap_or_default()),
            Expr::Send(s) => {
                let payload = interpolate(s, &self.scope).await;
                self.send_bytes(format!("{payload}\n").as_bytes(), expr.span.clone())
                    .await?;
                self.emit_event(LogEventKind::Send { data: payload.clone() }).await;
                self.emit_progress(ProgressEvent::Send);
                Ok(payload)
            }
            Expr::SendRaw(s) => {
                let payload = interpolate(s, &self.scope).await;
                self.send_bytes(payload.as_bytes(), expr.span.clone())
                    .await?;
                self.emit_event(LogEventKind::Send { data: payload.clone() }).await;
                self.emit_progress(ProgressEvent::Send);
                Ok(payload)
            }
            Expr::MatchLiteral(m) => {
                let timeout = m.timeout_override.unwrap_or_else(|| self.scope.timeout());
                let pattern = interpolate(&m.pattern, &self.scope).await;
                self.emit_event(LogEventKind::MatchStart { pattern: pattern.clone(), is_regex: false }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (start, end) = self.wait_for_literal(&pattern, timeout, expr.span.clone()).await?;
                let buffer = self.buffer_snapshot_match(start, end).await;
                self.emit_event(LogEventKind::MatchDone { matched: pattern.clone(), elapsed: match_start.elapsed(), buffer }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.cursor = end.max(start);
                Ok(pattern)
            }
            Expr::MatchRegex(m) => {
                let timeout = m.timeout_override.unwrap_or_else(|| self.scope.timeout());
                let pattern = interpolate(&m.pattern, &self.scope).await;
                let re = RegexBuilder::new(&pattern).multi_line(true).crlf(true).build().map_err(|e| Failure::Runtime {
                    message: format!("invalid regex: {}", regex_error_summary(&e)),
                    span: Some(m.pattern.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                self.emit_event(LogEventKind::MatchStart { pattern: pattern.clone(), is_regex: true }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (start, end, captures) = self
                    .wait_for_regex(&pattern, &re, timeout, expr.span.clone())
                    .await?;
                let full = captures.get("0").cloned().unwrap_or_default();
                let buffer = self.buffer_snapshot_match(start, end).await;
                self.emit_event(LogEventKind::MatchDone { matched: full.clone(), elapsed: match_start.elapsed(), buffer }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.cursor = end;
                self.scope.set_captures(captures);
                Ok(full)
            }
            Expr::NegMatchLiteral(m) => {
                let timeout = m.timeout_override.unwrap_or_else(|| self.scope.timeout());
                let pattern = interpolate(&m.pattern, &self.scope).await;
                self.emit_event(LogEventKind::NegMatchStart { pattern: pattern.clone(), is_regex: false }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                self.wait_for_absent_literal(&pattern, timeout, expr.span.clone()).await?;
                self.emit_event(LogEventKind::NegMatchPass { pattern: pattern.clone(), elapsed: match_start.elapsed() }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(String::new())
            }
            Expr::NegMatchRegex(m) => {
                let timeout = m.timeout_override.unwrap_or_else(|| self.scope.timeout());
                let pattern = interpolate(&m.pattern, &self.scope).await;
                let re = RegexBuilder::new(&pattern).multi_line(true).crlf(true).build().map_err(|e| Failure::Runtime {
                    message: format!("invalid regex: {}", regex_error_summary(&e)),
                    span: Some(m.pattern.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                self.emit_event(LogEventKind::NegMatchStart { pattern: pattern.clone(), is_regex: true }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                self.wait_for_absent_regex(&pattern, &re, timeout, expr.span.clone()).await?;
                self.emit_event(LogEventKind::NegMatchPass { pattern: pattern.clone(), elapsed: match_start.elapsed() }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(String::new())
            }
            Expr::BufferReset => {
                let buffer = self.buffer_snapshot_tail().await;
                let data = self.output_buf.snapshot().await;
                self.cursor = data.len();
                self.emit_event(LogEventKind::BufferReset { buffer }).await;
                Ok(String::new())
            }
            Expr::Call(call) => self.eval_call(call, &expr.span).await,
        }
    }

    async fn eval_call(&mut self, call: &ir::FnCall, span: &Span) -> Result<String, Failure> {
        let callable = self
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

        let mut evaluated_args = Vec::with_capacity(call.args.len());
        for arg in &call.args {
            evaluated_args.push(self.eval_expr(arg).await?);
        }

        match callable {
            Callable::UserDefined(fn_id) => {
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

                self.emit_event(LogEventKind::FnEnter { name: call.name.node.clone() }).await;
                self.emit_progress(ProgressEvent::FnEnter(call.name.node.clone()));
                let mut fn_vars = HashMap::new();
                for (param, value) in params.iter().zip(evaluated_args.into_iter()) {
                    fn_vars.insert(param.node.clone(), value);
                }
                let save = self.scope.enter_function(fn_vars);
                let mut last = String::new();
                for stmt in &body {
                    match self.exec_stmt(stmt).await {
                        Ok(v) => last = v,
                        Err(e) => {
                            self.scope.exit_function(save);
                            let _ = self.fail_watcher_tx.send(self.scope.fail_pattern().cloned());
                            return Err(e);
                        }
                    }
                }
                self.scope.exit_function(save);
                let _ = self.fail_watcher_tx.send(self.scope.fail_pattern().cloned());
                self.emit_event(LogEventKind::FnExit).await;
                self.emit_progress(ProgressEvent::FnExit);
                Ok(last)
            }
            Callable::UserDefinedPure(fn_id) => {
                let func = self
                    .code_server
                    .get_pure(fn_id)
                    .ok_or_else(|| Failure::Runtime {
                        message: format!("invalid pure function id {fn_id}"),
                        span: Some(span.clone()),
                        shell: Some(self.shell_name.clone()),
                    })?
                    .clone();

                self.emit_event(LogEventKind::FnEnter { name: call.name.node.clone() }).await;
                self.emit_progress(ProgressEvent::FnEnter(call.name.node.clone()));
                let mut fn_vars = HashMap::new();
                for (param, value) in func.params.iter().zip(evaluated_args.into_iter()) {
                    fn_vars.insert(param.node.clone(), value);
                }
                let env = self.scope.env();
                let cs = self.code_server.clone();
                let result = crate::runtime::pure::exec_pure_body(
                    &func.body,
                    &mut fn_vars,
                    &env,
                    &cs,
                    self,
                ).await;
                self.emit_event(LogEventKind::FnExit).await;
                self.emit_progress(ProgressEvent::FnExit);
                result
            }
            Callable::Builtin(bif) => {
                bif.call(self, evaluated_args, span).await
            }
            Callable::PureBuiltin(bif) => {
                bif.call(self, evaluated_args, span).await
            }
        }
    }

    async fn wait_for_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: Span,
    ) -> Result<(usize, usize), Failure> {
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

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.buffer_snapshot_tail().await;
                self.emit_event(LogEventKind::Timeout { pattern: pattern.to_string(), buffer }).await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.shell_name.clone(),
                })
            }
        }
    }

    async fn wait_for_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: Duration,
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

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.buffer_snapshot_tail().await;
                self.emit_event(LogEventKind::Timeout { pattern: pattern.to_string(), buffer }).await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.shell_name.clone(),
                })
            }
        }
    }

    async fn wait_for_absent_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: Span,
    ) -> Result<(), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some((start, end)) = self
                    .output_buf
                    .find_literal_from(self.cursor, pattern)
                    .await
                {
                    let buffer = self.buffer_snapshot_match(start, end).await;
                    self.emit_event(LogEventKind::NegMatchFail {
                        pattern: pattern.to_string(),
                        matched_text: pattern.to_string(),
                        buffer,
                    }).await;
                    return Err(Failure::NegativeMatchFailed {
                        pattern: pattern.to_string(),
                        matched_text: pattern.to_string(),
                        span,
                        shell: self.shell_name.clone(),
                    });
                }
                self.output_buf.notify.notified().await;
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => Ok(()),
        }
    }

    async fn wait_for_absent_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: Duration,
        span: Span,
    ) -> Result<(), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some((start, end, captures)) =
                    self.output_buf.find_regex_from(self.cursor, re).await
                {
                    let matched_text = captures.get("0").cloned().unwrap_or_default();
                    let buffer = self.buffer_snapshot_match(start, end).await;
                    self.emit_event(LogEventKind::NegMatchFail {
                        pattern: pattern.to_string(),
                        matched_text: matched_text.clone(),
                        buffer,
                    }).await;
                    return Err(Failure::NegativeMatchFailed {
                        pattern: pattern.to_string(),
                        matched_text,
                        span,
                        shell: self.shell_name.clone(),
                    });
                }
                self.output_buf.notify.notified().await;
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => Ok(()),
        }
    }

    async fn check_fail(&self, span: Span) -> Result<(), Failure> {
        if self.fail_triggered.load(Ordering::Relaxed) {
            self.emit_progress(ProgressEvent::FailPattern);
            let detail = self.fail_detail.lock().await.clone().unwrap_or_default();
            let buffer = self.buffer_snapshot_tail().await;
            self.emit_event(LogEventKind::FailPatternTriggered {
                pattern: detail.0.clone(),
                matched_line: detail.1.clone(),
                buffer,
            }).await;
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
            })?;
        self.shell_log.lock().await.log_stdin(data);
        Ok(())
    }

    pub async fn output_snapshot(&self) -> Vec<u8> {
        self.output_buf.snapshot().await
    }

    pub async fn reset_for_reuse(&mut self, timeout: Duration) {
        self.scope.set_timeout(timeout);
        self.scope.set_fail_pattern(None);
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

impl Vm {
    async fn emit_event(&self, kind: LogEventKind) {
        if let Some(ec) = &self.event_collector {
            ec.push(&self.shell_name, kind).await;
        }
    }

    async fn buffer_snapshot_match(&self, start: usize, end: usize) -> BufferSnapshot {
        let data = self.output_buf.snapshot().await;
        let text = String::from_utf8_lossy(&data);
        let before_raw = &text[self.cursor.min(text.len())..start.min(text.len())];
        let matched_raw = &text[start.min(text.len())..end.min(text.len())];
        let after_raw = &text[end.min(text.len())..];
        BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: matched_raw.to_string(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        }
    }

    async fn buffer_snapshot_tail(&self) -> BufferSnapshot {
        let data = self.output_buf.snapshot().await;
        let text = String::from_utf8_lossy(&data);
        let from_cursor = &text[self.cursor.min(text.len())..];
        BufferSnapshot::Tail {
            content: truncate_before(from_cursor, BUFFER_TAIL_LEN),
        }
    }
}

#[async_trait::async_trait]
impl PureContext for Vm {
    fn emit_progress(&self, event: ProgressEvent) {
        if let Some(tx) = &self.progress_tx {
            let _ = tx.send(event);
        }
    }

    async fn emit_log(&mut self, message: String) {
        self.emit_event(LogEventKind::Log { message }).await;
    }
}

#[async_trait::async_trait]
impl VmContext for Vm {
    async fn match_literal(&mut self, pattern: &str, span: &Span) -> Result<String, Failure> {
        self.emit_event(LogEventKind::MatchStart { pattern: pattern.to_string(), is_regex: false }).await;
        self.emit_progress(ProgressEvent::MatchStart);
        let match_start = Instant::now();
        let timeout = self.scope.timeout();
        let (start, end) = self.wait_for_literal(pattern, timeout, span.clone()).await?;
        let buffer = self.buffer_snapshot_match(start, end).await;
        self.emit_event(LogEventKind::MatchDone { matched: pattern.to_string(), elapsed: match_start.elapsed(), buffer }).await;
        self.emit_progress(ProgressEvent::MatchDone);
        self.cursor = end.max(start);
        Ok(pattern.to_string())
    }

    async fn send_line(&mut self, line: &str, span: &Span) -> Result<(), Failure> {
        self.send_bytes(format!("{line}\n").as_bytes(), span.clone()).await?;
        self.emit_event(LogEventKind::Send { data: line.to_string() }).await;
        self.emit_progress(ProgressEvent::Send);
        Ok(())
    }

    async fn send_raw(&mut self, data: &[u8], span: &Span) -> Result<(), Failure> {
        self.send_bytes(data, span.clone()).await?;
        let display = data.iter().map(|b| format!("\\x{b:02x}")).collect::<String>();
        self.emit_event(LogEventKind::Send { data: display }).await;
        self.emit_progress(ProgressEvent::Send);
        Ok(())
    }

    fn shell_prompt(&self) -> &str {
        &self.shell_prompt
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
