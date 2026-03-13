use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::BytesMut;
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

// ─── Match Types ────────────────────────────────────────────────

/// Marker trait for match payload types.
pub trait MatchKind {}

/// Payload for a literal match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralMatch(pub String);
impl MatchKind for LiteralMatch {}

/// Payload for a regex match (capture groups by index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch(pub HashMap<String, String>);
impl MatchKind for RegexMatch {}

/// A match result with absolute byte offsets and typed payload.
#[derive(Debug, Clone)]
pub struct Match<T: MatchKind> {
    /// Absolute byte offset of match start (accounts for all prior truncations).
    pub start: usize,
    /// Absolute byte offset of match end.
    pub end: usize,
    /// Bytes consumed (everything up to and including the match, relative to current buffer).
    pub consumed: usize,
    /// The matched content.
    pub value: T,
}

// ─── OutputBuffer ───────────────────────────────────────────────

struct BufferInner {
    data: BytesMut,
    base: usize,
}

#[derive(Clone)]
pub struct OutputBuffer {
    inner: Arc<Mutex<BufferInner>>,
    pub(crate) notify: Arc<Notify>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                data: BytesMut::new(),
                base: 0,
            })),
            notify: Arc::new(Notify::new()),
        }
    }

    pub async fn append(&self, bytes: &[u8]) {
        self.inner.lock().await.data.extend_from_slice(bytes);
        self.notify.notify_waiters();
    }

    /// Find literal, extract truncated context, drain via split_to. One lock.
    /// Returns Match + BufferSnapshot for event emission.
    pub async fn consume_literal(
        &self,
        needle: &str,
    ) -> Option<(Match<LiteralMatch>, BufferSnapshot)> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let pos = text.find(needle)?;
        let end_pos = pos + needle.len();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: needle.to_string(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: LiteralMatch(needle.to_string()),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Some((m, snapshot))
    }

    /// Find regex, extract truncated context, drain via split_to. One lock.
    pub async fn consume_regex(
        &self,
        re: &Regex,
    ) -> Option<(Match<RegexMatch>, BufferSnapshot)> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let cap = re.captures(&text)?;
        let whole = cap.get(0)?;
        let pos = whole.start();
        let end_pos = whole.end();
        let matched_str = whole.as_str().to_string();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: matched_str.clone(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let mut captures = HashMap::new();
        for i in 0..cap.len() {
            if let Some(m) = cap.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }

        let consumed = end_pos;
        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed,
            value: RegexMatch(captures),
        };

        drop(text);
        let _ = inner.data.split_to(end_pos);
        inner.base += end_pos;

        Some((m, snapshot))
    }

    /// Find literal, extract truncated context, NO drain. One lock.
    pub async fn peek_literal(
        &self,
        needle: &str,
    ) -> Option<(Match<LiteralMatch>, BufferSnapshot)> {
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let pos = text.find(needle)?;
        let end_pos = pos + needle.len();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: needle.to_string(),
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed: end_pos,
            value: LiteralMatch(needle.to_string()),
        };

        Some((m, snapshot))
    }

    /// Find regex, extract truncated context, NO drain. One lock.
    pub async fn peek_regex(
        &self,
        re: &Regex,
    ) -> Option<(Match<RegexMatch>, BufferSnapshot)> {
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        let cap = re.captures(&text)?;
        let whole = cap.get(0)?;
        let pos = whole.start();
        let end_pos = whole.end();
        let matched_str = whole.as_str().to_string();

        let before_raw = &text[..pos];
        let after_raw = &text[end_pos..];
        let snapshot = BufferSnapshot::Match {
            before: truncate_before(before_raw, BUFFER_PREFIX_LEN),
            matched: matched_str,
            after: truncate_after(after_raw, BUFFER_SUFFIX_LEN),
        };

        let mut captures = HashMap::new();
        for i in 0..cap.len() {
            if let Some(m) = cap.get(i) {
                captures.insert(i.to_string(), m.as_str().to_string());
            }
        }

        let m = Match {
            start: inner.base + pos,
            end: inner.base + end_pos,
            consumed: end_pos,
            value: RegexMatch(captures),
        };

        Some((m, snapshot))
    }

    /// Drain all buffered data, advancing base.
    pub async fn clear(&self) {
        let mut inner = self.inner.lock().await;
        let len = inner.data.len();
        let _ = inner.data.split_to(len);
        inner.base += len;
    }

    /// Return a BufferSnapshot::Tail of the current buffer (last `n` chars).
    pub async fn snapshot_tail(&self, n: usize) -> BufferSnapshot {
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        BufferSnapshot::Tail {
            content: truncate_before(&text, n),
        }
    }

    /// Return remaining buffer data (for shell_logs in TestResult).
    pub async fn remaining(&self) -> Vec<u8> {
        let inner = self.inner.lock().await;
        inner.data.to_vec()
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
                if let Some((_m, _snapshot)) = self
                    .output_buf
                    .consume_regex(&prompt_re)
                    .await
                {
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
                let (mat, snapshot) = self.wait_consume_literal(&pattern, timeout, expr.span.clone()).await?;
                self.emit_event(LogEventKind::MatchDone { matched: mat.value.0.clone(), elapsed: match_start.elapsed(), buffer: snapshot }).await;
                self.emit_progress(ProgressEvent::MatchDone);
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
                let (mat, snapshot) = self
                    .wait_consume_regex(&pattern, &re, timeout, expr.span.clone())
                    .await?;
                let full = mat.value.0.get("0").cloned().unwrap_or_default();
                self.emit_event(LogEventKind::MatchDone { matched: full.clone(), elapsed: match_start.elapsed(), buffer: snapshot }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.scope.set_captures(mat.value.0);
                Ok(full)
            }
            Expr::NegMatchLiteral(m) => {
                let timeout = m.timeout_override.unwrap_or_else(|| self.scope.timeout());
                let pattern = interpolate(&m.pattern, &self.scope).await;
                self.emit_event(LogEventKind::NegMatchStart { pattern: pattern.clone(), is_regex: false }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                self.wait_absent_literal(&pattern, timeout, expr.span.clone()).await?;
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
                self.wait_absent_regex(&pattern, &re, timeout, expr.span.clone()).await?;
                self.emit_event(LogEventKind::NegMatchPass { pattern: pattern.clone(), elapsed: match_start.elapsed() }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(String::new())
            }
            Expr::BufferReset => {
                let snapshot = self.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::BufferReset { buffer: snapshot }).await;
                self.output_buf.clear().await;
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

    // ─── Wait + consume/peek helpers ────────────────────────────

    async fn wait_consume_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: Span,
    ) -> Result<(Match<LiteralMatch>, BufferSnapshot), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some(result) = self.output_buf.consume_literal(pattern).await {
                    return Ok::<(Match<LiteralMatch>, BufferSnapshot), Failure>(result);
                }
                self.output_buf.notify.notified().await;
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::Timeout { pattern: pattern.to_string(), buffer }).await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.shell_name.clone(),
                })
            }
        }
    }

    async fn wait_consume_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: Duration,
        span: Span,
    ) -> Result<(Match<RegexMatch>, BufferSnapshot), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some(result) = self.output_buf.consume_regex(re).await {
                    return Ok::<(Match<RegexMatch>, BufferSnapshot), Failure>(result);
                }
                self.output_buf.notify.notified().await;
            }
        };

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => {
                self.emit_progress(ProgressEvent::Timeout);
                let buffer = self.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
                self.emit_event(LogEventKind::Timeout { pattern: pattern.to_string(), buffer }).await;
                Err(Failure::MatchTimeout {
                    pattern: pattern.to_string(),
                    span,
                    shell: self.shell_name.clone(),
                })
            }
        }
    }

    async fn wait_absent_literal(
        &self,
        pattern: &str,
        timeout: Duration,
        span: Span,
    ) -> Result<(), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some((_m, snapshot)) = self.output_buf.peek_literal(pattern).await {
                    self.emit_event(LogEventKind::NegMatchFail {
                        pattern: pattern.to_string(),
                        matched_text: pattern.to_string(),
                        buffer: snapshot,
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

    async fn wait_absent_regex(
        &self,
        pattern: &str,
        re: &Regex,
        timeout: Duration,
        span: Span,
    ) -> Result<(), Failure> {
        let fut = async {
            loop {
                self.check_fail(span.clone()).await?;
                if let Some((m, snapshot)) = self.output_buf.peek_regex(re).await {
                    let matched_text = m.value.0.get("0").cloned().unwrap_or_default();
                    self.emit_event(LogEventKind::NegMatchFail {
                        pattern: pattern.to_string(),
                        matched_text: matched_text.clone(),
                        buffer: snapshot,
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
            let buffer = self.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
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
        self.output_buf.remaining().await
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
        let (mat, snapshot) = self.wait_consume_literal(pattern, timeout, span.clone()).await?;
        self.emit_event(LogEventKind::MatchDone { matched: mat.value.0.clone(), elapsed: match_start.elapsed(), buffer: snapshot }).await;
        self.emit_progress(ProgressEvent::MatchDone);
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
        loop {
            let current = rx.borrow().clone();
            if let Some(pattern) = current {
                loop {
                    let matched = match &pattern {
                        FailPattern::Regex(re) => {
                            output.peek_regex(re).await.map(|(m, _snapshot)| {
                                let matched_text = m.value.0.get("0").cloned().unwrap_or_default();
                                (re.as_str().to_string(), matched_text)
                            })
                        }
                        FailPattern::Literal(s) => {
                            output.peek_literal(s).await.map(|(_m, _snapshot)| {
                                (s.clone(), s.clone())
                            })
                        }
                    };
                    if let Some((p, line)) = matched {
                        flag.store(true, Ordering::Relaxed);
                        *detail.lock().await = Some((p, line));
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── truncate_before ──────────────────────────────────────────────

    #[test]
    fn truncate_before_short_string_unchanged() {
        assert_eq!(truncate_before("hello", 10), "hello");
    }

    #[test]
    fn truncate_before_exact_length_unchanged() {
        assert_eq!(truncate_before("hello", 5), "hello");
    }

    #[test]
    fn truncate_before_keeps_last_n_chars() {
        assert_eq!(truncate_before("hello world", 5), "...world");
    }

    #[test]
    fn truncate_before_empty_string() {
        assert_eq!(truncate_before("", 5), "");
    }

    #[test]
    fn truncate_before_max_zero() {
        assert_eq!(truncate_before("hello", 0), "...");
    }

    // ── truncate_after ───────────────────────────────────────────────

    #[test]
    fn truncate_after_short_string_unchanged() {
        assert_eq!(truncate_after("hello", 10), "hello");
    }

    #[test]
    fn truncate_after_exact_length_unchanged() {
        assert_eq!(truncate_after("hello", 5), "hello");
    }

    #[test]
    fn truncate_after_keeps_first_n_chars() {
        assert_eq!(truncate_after("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_after_empty_string() {
        assert_eq!(truncate_after("", 5), "");
    }

    #[test]
    fn truncate_after_max_zero() {
        assert_eq!(truncate_after("hello", 0), "...");
    }

    // ── regex_error_summary ──────────────────────────────────────────

    #[test]
    fn regex_error_summary_extracts_last_line() {
        let err = Regex::new("(unclosed").unwrap_err();
        let summary = regex_error_summary(&err);
        assert!(!summary.is_empty());
        assert!(!summary.starts_with("error: "));
    }

    #[test]
    fn regex_error_summary_strips_error_prefix() {
        let err = Regex::new("[invalid").unwrap_err();
        let summary = regex_error_summary(&err);
        assert!(!summary.starts_with("error: "));
    }

    // ── OutputBuffer::new ────────────────────────────────────────────

    #[tokio::test]
    async fn output_buffer_new_is_empty() {
        let buf = OutputBuffer::new();
        assert!(buf.remaining().await.is_empty());
    }

    // ── OutputBuffer::append + remaining ─────────────────────────────

    #[tokio::test]
    async fn output_buffer_append_and_remaining() {
        let buf = OutputBuffer::new();
        buf.append(b"hello ").await;
        buf.append(b"world").await;
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn output_buffer_append_empty_bytes() {
        let buf = OutputBuffer::new();
        buf.append(b"").await;
        assert!(buf.remaining().await.is_empty());
    }

    // ── OutputBuffer::consume_literal ────────────────────────────────

    #[tokio::test]
    async fn consume_literal_basic() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let (m, snapshot) = buf.consume_literal("hello").await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        assert_eq!(m.consumed, 5);
        assert_eq!(m.value.0, "hello");
        assert!(matches!(snapshot, BufferSnapshot::Match { .. }));
        // Buffer should have " world" remaining
        assert_eq!(buf.remaining().await, b" world");
    }

    #[tokio::test]
    async fn consume_literal_drains_up_to_match_end() {
        let buf = OutputBuffer::new();
        buf.append(b"prefix MATCH suffix").await;
        let (m, _) = buf.consume_literal("MATCH").await.unwrap();
        assert_eq!(m.start, 7);
        assert_eq!(m.end, 12);
        assert_eq!(m.consumed, 12);
        assert_eq!(buf.remaining().await, b" suffix");
    }

    #[tokio::test]
    async fn consume_literal_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        assert!(buf.consume_literal("xyz").await.is_none());
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_literal_absolute_offsets_after_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"aaa bbb ccc").await;
        // Consume "aaa"
        let (m1, _) = buf.consume_literal("aaa").await.unwrap();
        assert_eq!(m1.start, 0);
        assert_eq!(m1.end, 3);
        // Now consume "bbb" — absolute offsets should account for drained bytes
        let (m2, _) = buf.consume_literal("bbb").await.unwrap();
        assert_eq!(m2.start, 4);
        assert_eq!(m2.end, 7);
        // Remaining should be " ccc"
        assert_eq!(buf.remaining().await, b" ccc");
    }

    #[tokio::test]
    async fn consume_literal_snapshot_has_truncated_context() {
        let buf = OutputBuffer::new();
        buf.append(b"before MATCH after").await;
        let (_, snapshot) = buf.consume_literal("MATCH").await.unwrap();
        match snapshot {
            BufferSnapshot::Match { before, matched, after } => {
                assert_eq!(before, "before ");
                assert_eq!(matched, "MATCH");
                assert_eq!(after, " after");
            }
            _ => panic!("expected BufferSnapshot::Match"),
        }
    }

    // ── OutputBuffer::consume_regex ──────────────────────────────────

    #[tokio::test]
    async fn consume_regex_basic() {
        let buf = OutputBuffer::new();
        buf.append(b"abc 123 def").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert_eq!(m.value.0.get("0").unwrap(), "123");
        assert_eq!(buf.remaining().await, b" def");
    }

    #[tokio::test]
    async fn consume_regex_with_captures() {
        let buf = OutputBuffer::new();
        buf.append(b"name: Alice age: 30").await;
        let re = Regex::new(r"name: (\w+) age: (\d+)").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 19);
        assert_eq!(m.value.0.get("0").unwrap(), "name: Alice age: 30");
        assert_eq!(m.value.0.get("1").unwrap(), "Alice");
        assert_eq!(m.value.0.get("2").unwrap(), "30");
        assert!(buf.remaining().await.is_empty());
    }

    #[tokio::test]
    async fn consume_regex_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let re = Regex::new(r"\d+").unwrap();
        assert!(buf.consume_regex(&re).await.is_none());
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn consume_regex_absolute_offsets_after_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"aaa 123 bbb 456").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m1, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m1.start, 4);
        assert_eq!(m1.end, 7);
        // After consuming "aaa 123", buffer has " bbb 456"
        let (m2, _) = buf.consume_regex(&re).await.unwrap();
        assert_eq!(m2.start, 12);
        assert_eq!(m2.end, 15);
    }

    // ── OutputBuffer::peek_literal ──────────────────────────────────

    #[tokio::test]
    async fn peek_literal_does_not_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let (m, _) = buf.peek_literal("hello").await.unwrap();
        assert_eq!(m.start, 0);
        assert_eq!(m.end, 5);
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"hello world");
    }

    #[tokio::test]
    async fn peek_literal_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello").await;
        assert!(buf.peek_literal("xyz").await.is_none());
    }

    // ── OutputBuffer::peek_regex ────────────────────────────────────

    #[tokio::test]
    async fn peek_regex_does_not_drain() {
        let buf = OutputBuffer::new();
        buf.append(b"abc 123 def").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.peek_regex(&re).await.unwrap();
        assert_eq!(m.start, 4);
        assert_eq!(m.end, 7);
        assert_eq!(m.value.0.get("0").unwrap(), "123");
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"abc 123 def");
    }

    // ── OutputBuffer::clear ─────────────────────────────────────────

    #[tokio::test]
    async fn clear_empties_buffer() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        buf.clear().await;
        assert!(buf.remaining().await.is_empty());
    }

    #[tokio::test]
    async fn clear_advances_base_correctly() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        buf.clear().await;
        buf.append(b"abc 123").await;
        let re = Regex::new(r"\d+").unwrap();
        let (m, _) = buf.consume_regex(&re).await.unwrap();
        // base should be 11 (from clear) + 4 (from "abc ") = absolute offset 15
        assert_eq!(m.start, 15);
        assert_eq!(m.end, 18);
    }

    // ── OutputBuffer::snapshot_tail ─────────────────────────────────

    #[tokio::test]
    async fn snapshot_tail_returns_tail() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let snapshot = buf.snapshot_tail(5).await;
        match snapshot {
            BufferSnapshot::Tail { content } => {
                assert_eq!(content, "...world");
            }
            _ => panic!("expected Tail"),
        }
    }

    #[tokio::test]
    async fn snapshot_tail_full_content_when_short() {
        let buf = OutputBuffer::new();
        buf.append(b"hi").await;
        let snapshot = buf.snapshot_tail(80).await;
        match snapshot {
            BufferSnapshot::Tail { content } => {
                assert_eq!(content, "hi");
            }
            _ => panic!("expected Tail"),
        }
    }
}
