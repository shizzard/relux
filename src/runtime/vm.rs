use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use regex::{Regex, RegexBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::{Mutex, Notify};

use crate::dsl::resolver::ir::{self, Expr, ShellStmt, Span, Spanned, StringExpr, StringPart};
use crate::runtime::event_log::{BufferSnapshot, EventCollector, LogEventKind};
use crate::runtime::result::Failure;
use crate::runtime::shell_log::ShellLogger;
use crate::runtime::vars::{FailPattern, ScopeStack, interpolate};
use crate::runtime::bifs::{PureContext, VmContext};
use crate::runtime::progress::{ProgressEvent, ProgressTx};
use crate::runtime::{Callable, CodeServer};

/// A fail pattern matched in the output buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailPatternHit {
    /// The pattern string that was being watched for (regex source or literal).
    pattern: String,
    /// The actual text in the buffer that matched.
    matched_text: String,
}

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

// ─── Interpolation helpers ──────────────────────────────────────

fn has_interpolation(expr: &StringExpr) -> bool {
    expr.parts.iter().any(|p| matches!(p.node, StringPart::Interp(_)))
}

fn interpolation_template(expr: &StringExpr) -> String {
    expr.parts.iter().map(|p| match &p.node {
        StringPart::Literal(s) => s.clone(),
        StringPart::Interp(name) => format!("${{{name}}}"),
        StringPart::EscapedDollar => "$".to_string(),
    }).collect()
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
    recv_pending: Arc<Mutex<BytesMut>>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                data: BytesMut::new(),
                base: 0,
            })),
            notify: Arc::new(Notify::new()),
            recv_pending: Arc::new(Mutex::new(BytesMut::new())),
        }
    }

    pub async fn append(&self, bytes: &[u8]) {
        self.inner.lock().await.data.extend_from_slice(bytes);
        self.recv_pending.lock().await.extend_from_slice(bytes);
        self.notify.notify_waiters();
    }

    pub async fn drain_recv(&self) -> Option<String> {
        let mut pending = self.recv_pending.lock().await;
        if pending.is_empty() {
            return None;
        }
        let bytes = pending.split();
        Some(String::from_utf8_lossy(&bytes).to_string())
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

    /// Check fail pattern against buffer, then try to consume literal — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if literal consumed, Ok(None) if not found.
    pub async fn fail_check_consume_literal(
        &self,
        needle: &str,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<LiteralMatch>, BufferSnapshot)>, FailPatternHit> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);

        // Check fail pattern first
        if let Some(fp) = fail_pattern {
            if let Some(hit) = check_fail_in_buffer(&text, fp) {
                return Err(hit);
            }
        }

        // Try to consume the literal
        let Some(pos) = text.find(needle) else {
            return Ok(None);
        };
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

        Ok(Some((m, snapshot)))
    }

    /// Check fail pattern against buffer, then try to consume regex — under one lock.
    /// Returns Err if fail pattern found, Ok(Some) if regex consumed, Ok(None) if not found.
    pub async fn fail_check_consume_regex(
        &self,
        re: &Regex,
        fail_pattern: Option<&FailPattern>,
    ) -> Result<Option<(Match<RegexMatch>, BufferSnapshot)>, FailPatternHit> {
        let mut inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);

        // Check fail pattern first
        if let Some(fp) = fail_pattern {
            if let Some(hit) = check_fail_in_buffer(&text, fp) {
                return Err(hit);
            }
        }

        // Try to consume the regex
        let Some(cap) = re.captures(&text) else {
            return Ok(None);
        };
        let Some(whole) = cap.get(0) else {
            return Ok(None);
        };
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

        Ok(Some((m, snapshot)))
    }

    /// Check fail pattern against current buffer (peek only, no drain).
    pub async fn check_fail_pattern(
        &self,
        fail_pattern: Option<&FailPattern>,
    ) -> Option<FailPatternHit> {
        let fp = fail_pattern?;
        let inner = self.inner.lock().await;
        let text = String::from_utf8_lossy(&inner.data);
        check_fail_in_buffer(&text, fp)
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

/// Check if a fail pattern matches in the given text. Returns (pattern_str, matched_text).
fn check_fail_in_buffer(text: &str, pattern: &FailPattern) -> Option<FailPatternHit> {
    match pattern {
        FailPattern::Regex(re) => {
            let m = re.find(text)?;
            Some(FailPatternHit {
                pattern: re.as_str().to_string(),
                matched_text: m.as_str().to_string(),
            })
        }
        FailPattern::Literal(s) => {
            text.find(s.as_str())?;
            Some(FailPatternHit {
                pattern: s.clone(),
                matched_text: s.clone(),
            })
        }
    }
}

pub struct Vm {
    writer: pty_process::OwnedWritePty,
    child: Child,
    output_buf: OutputBuffer,
    read_task: tokio::task::JoinHandle<()>,
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

        let mut vm = Self {
            writer,
            child,
            output_buf,
            read_task,
            scope,
            code_server,
            shell_name: shell_name.clone(),
            shell_prompt,
            progress_tx,
            shell_log,
            event_collector,
        };

        vm.emit_event(LogEventKind::ShellSpawn {
            name: shell_name.clone(),
            command: shell_command,
        }).await;

        vm.init_prompt().await.map_err(|_| Failure::Runtime {
            message: "shell did not produce prompt during init".to_string(),
            span: None,
            shell: Some(shell_name),
        })?;

        vm.emit_event(LogEventKind::ShellReady { name: vm.shell_name.clone() }).await;

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

        tokio::time::timeout(self.scope.timeout().resolve(), async {
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
        self.drain_recv_event().await;
        Ok(last)
    }

    async fn drain_recv_event(&mut self) {
        if let Some(_data) = self.output_buf.drain_recv().await {
            // self.emit_event(LogEventKind::Recv { data }).await;
        }
    }

    async fn emit_interpolation(&mut self, expr: &StringExpr, result: &str) {
        if has_interpolation(expr) {
            let mut bindings = Vec::new();
            for part in &expr.parts {
                if let StringPart::Interp(name) = &part.node {
                    let value = self.scope.lookup(name).await.unwrap_or_default();
                    bindings.push((name.clone(), value));
                }
            }
            self.emit_event(LogEventKind::Interpolation {
                template: interpolation_template(expr),
                result: result.to_string(),
                bindings,
            }).await;
        }
    }

    pub async fn exec_stmt(&mut self, stmt: &Spanned<ShellStmt>) -> Result<String, Failure> {
        self.drain_recv_event().await;
        self.check_fail(stmt.span.clone()).await?;
        match &stmt.node {
            ShellStmt::FailRegex(expr) => {
                let pat = interpolate(expr, &self.scope).await;
                self.emit_interpolation(expr, &pat).await;
                self.emit_event(LogEventKind::FailPatternSet { pattern: pat.clone() }).await;
                let re = RegexBuilder::new(&pat).multi_line(true).crlf(true).build().map_err(|e| Failure::Runtime {
                    message: format!("invalid fail regex: {}", regex_error_summary(&e)),
                    span: Some(expr.span.clone()),
                    shell: Some(self.shell_name.clone()),
                })?;
                let pattern = Some(FailPattern::Regex(re));
                self.scope.set_fail_pattern(pattern);
                // Immediately rescan buffer for the new pattern
                self.check_fail(stmt.span.clone()).await?;
                Ok(String::new())
            }
            ShellStmt::FailLiteral(expr) => {
                let pat = interpolate(expr, &self.scope).await;
                self.emit_interpolation(expr, &pat).await;
                self.emit_event(LogEventKind::FailPatternSet { pattern: pat.clone() }).await;
                let pattern = Some(FailPattern::Literal(pat));
                self.scope.set_fail_pattern(pattern);
                // Immediately rescan buffer for the new pattern
                self.check_fail(stmt.span.clone()).await?;
                Ok(String::new())
            }
            ShellStmt::ClearFailPattern => {
                self.emit_event(LogEventKind::FailPatternCleared).await;
                self.scope.set_fail_pattern(None);
                Ok(String::new())
            }
            ShellStmt::Timeout(t) => {
                let previous = format!("{:?}", self.scope.timeout());
                self.scope.set_timeout(t.clone());
                let timeout = format!("{:?}", self.scope.timeout());
                self.emit_event(LogEventKind::TimeoutSet { timeout, previous }).await;
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
            Expr::String(s) => {
                let result = interpolate(s, &self.scope).await;
                self.emit_interpolation(s, &result).await;
                self.emit_event(LogEventKind::StringEval { result: result.clone() }).await;
                Ok(result)
            }
            Expr::Var(name) => Ok(self.scope.lookup(name).await.unwrap_or_default()),
            Expr::Send(s) => {
                let payload = interpolate(s, &self.scope).await;
                self.emit_interpolation(s, &payload).await;
                self.send_bytes(format!("{payload}\n").as_bytes(), expr.span.clone())
                    .await?;
                self.emit_event(LogEventKind::Send { data: payload.clone() }).await;
                self.emit_progress(ProgressEvent::Send);
                Ok(payload)
            }
            Expr::SendRaw(s) => {
                let payload = interpolate(s, &self.scope).await;
                self.emit_interpolation(s, &payload).await;
                self.send_bytes(payload.as_bytes(), expr.span.clone())
                    .await?;
                self.emit_event(LogEventKind::Send { data: payload.clone() }).await;
                self.emit_progress(ProgressEvent::Send);
                Ok(payload)
            }
            Expr::MatchLiteral(m) => {
                let timeout = m.timeout_override.as_ref().unwrap_or_else(|| self.scope.timeout()).resolve();
                let pattern = interpolate(&m.pattern, &self.scope).await;
                self.emit_interpolation(&m.pattern, &pattern).await;
                self.emit_event(LogEventKind::MatchStart { pattern: pattern.clone(), is_regex: false }).await;
                self.emit_progress(ProgressEvent::MatchStart);
                let match_start = Instant::now();
                let (mat, snapshot) = self.wait_consume_literal(&pattern, timeout, expr.span.clone()).await?;
                self.emit_event(LogEventKind::MatchDone { matched: mat.value.0.clone(), elapsed: match_start.elapsed(), buffer: snapshot, captures: None }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                Ok(pattern)
            }
            Expr::MatchRegex(m) => {
                let timeout = m.timeout_override.as_ref().unwrap_or_else(|| self.scope.timeout()).resolve();
                let pattern = interpolate(&m.pattern, &self.scope).await;
                self.emit_interpolation(&m.pattern, &pattern).await;
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
                let captures = mat.value.0.clone();
                self.emit_event(LogEventKind::MatchDone { matched: full.clone(), elapsed: match_start.elapsed(), buffer: snapshot, captures: Some(captures.clone()) }).await;
                self.emit_progress(ProgressEvent::MatchDone);
                self.scope.set_captures(captures);
                Ok(full)
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

                let named_args: Vec<(String, String)> = params.iter().zip(evaluated_args.iter()).map(|(p, v)| (p.node.clone(), v.clone())).collect();
                self.emit_event(LogEventKind::FnEnter { name: call.name.node.clone(), args: named_args }).await;
                self.emit_progress(ProgressEvent::FnEnter(call.name.node.clone()));
                // Snapshot pre-call state for FnExit reporting
                let pre_timeout = format!("{:?}", self.scope.timeout());
                let pre_fail = self.scope.fail_pattern().map(|p| format!("{p:?}"));
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
                            return Err(e);
                        }
                    }
                }
                self.scope.exit_function(save);
                let post_timeout = format!("{:?}", self.scope.timeout());
                let post_fail = self.scope.fail_pattern().map(|p| format!("{p:?}"));
                self.emit_event(LogEventKind::FnExit {
                    name: call.name.node.clone(),
                    return_value: last.clone(),
                    restored_timeout: if post_timeout != pre_timeout { Some(post_timeout) } else { None },
                    restored_fail_pattern: if post_fail != pre_fail { post_fail } else { None },
                }).await;
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

                let named_args: Vec<(String, String)> = func.params.iter().zip(evaluated_args.iter()).map(|(p, v)| (p.node.clone(), v.clone())).collect();
                self.emit_event(LogEventKind::FnEnter { name: call.name.node.clone(), args: named_args }).await;
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
                let return_value = result.as_ref().map(|s| s.clone()).unwrap_or_default();
                self.emit_event(LogEventKind::FnExit {
                    name: call.name.node.clone(),
                    return_value,
                    restored_timeout: None,
                    restored_fail_pattern: None,
                }).await;
                self.emit_progress(ProgressEvent::FnExit);
                result
            }
            Callable::Builtin(bif) => {
                let name = bif.name().to_string();
                let positional_args: Vec<(String, String)> = evaluated_args.iter().enumerate().map(|(i, v)| (format!("${i}"), v.clone())).collect();
                self.emit_event(LogEventKind::FnEnter { name: name.clone(), args: positional_args }).await;
                let result = bif.call(self, evaluated_args, span).await;
                let return_value = result.as_ref().map(|s| s.clone()).unwrap_or_default();
                self.emit_event(LogEventKind::FnExit { name, return_value, restored_timeout: None, restored_fail_pattern: None }).await;
                result
            }
            Callable::PureBuiltin(bif) => {
                let name = bif.name().to_string();
                let positional_args: Vec<(String, String)> = evaluated_args.iter().enumerate().map(|(i, v)| (format!("${i}"), v.clone())).collect();
                self.emit_event(LogEventKind::FnEnter { name: name.clone(), args: positional_args }).await;
                let result = bif.call(self, evaluated_args, span).await;
                let return_value = result.as_ref().map(|s| s.clone()).unwrap_or_default();
                self.emit_event(LogEventKind::FnExit { name, return_value, restored_timeout: None, restored_fail_pattern: None }).await;
                result
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
                let fail_pat = self.scope.fail_pattern();
                match self.output_buf.fail_check_consume_literal(pattern, fail_pat).await {
                    Err(hit) => {
                        return Err(self.make_fail_pattern_error(hit, span.clone()).await);
                    }
                    Ok(Some(result)) => {
                        return Ok::<(Match<LiteralMatch>, BufferSnapshot), Failure>(result);
                    }
                    Ok(None) => {}
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
                let fail_pat = self.scope.fail_pattern();
                match self.output_buf.fail_check_consume_regex(re, fail_pat).await {
                    Err(hit) => {
                        return Err(self.make_fail_pattern_error(hit, span.clone()).await);
                    }
                    Ok(Some(result)) => {
                        return Ok::<(Match<RegexMatch>, BufferSnapshot), Failure>(result);
                    }
                    Ok(None) => {}
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

    async fn check_fail(&self, span: Span) -> Result<(), Failure> {
        let fail_pat = self.scope.fail_pattern();
        if let Some(hit) = self.output_buf.check_fail_pattern(fail_pat).await {
            return Err(self.make_fail_pattern_error(hit, span).await);
        }
        Ok(())
    }

    async fn make_fail_pattern_error(&self, hit: FailPatternHit, span: Span) -> Failure {
        self.emit_progress(ProgressEvent::FailPattern);
        let buffer = self.output_buf.snapshot_tail(BUFFER_TAIL_LEN).await;
        self.emit_event(LogEventKind::FailPatternTriggered {
            pattern: hit.pattern.clone(),
            matched_line: hit.matched_text.clone(),
            buffer,
        }).await;
        Failure::FailPatternMatched {
            pattern: hit.pattern,
            matched_line: hit.matched_text,
            span,
            shell: self.shell_name.clone(),
        }
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
        self.emit_event(LogEventKind::ShellTerminate { name: self.shell_name.clone() }).await;
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
        let timeout = self.scope.timeout().resolve();
        let (mat, snapshot) = self.wait_consume_literal(pattern, timeout, span.clone()).await?;
        self.emit_event(LogEventKind::MatchDone { matched: mat.value.0.clone(), elapsed: match_start.elapsed(), buffer: snapshot, captures: None }).await;
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

    // ── check_fail_in_buffer ────────────────────────────────────────

    #[test]
    fn check_fail_in_buffer_regex_match() {
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let hit = check_fail_in_buffer("some ERROR here", &fp).unwrap();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
    }

    #[test]
    fn check_fail_in_buffer_regex_no_match() {
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        assert!(check_fail_in_buffer("all good", &fp).is_none());
    }

    #[test]
    fn check_fail_in_buffer_literal_match() {
        let fp = FailPattern::Literal("FATAL".to_string());
        let hit = check_fail_in_buffer("got FATAL crash", &fp).unwrap();
        assert_eq!(hit.pattern, "FATAL");
        assert_eq!(hit.matched_text, "FATAL");
    }

    #[test]
    fn check_fail_in_buffer_literal_no_match() {
        let fp = FailPattern::Literal("FATAL".to_string());
        assert!(check_fail_in_buffer("all good", &fp).is_none());
    }

    // ── OutputBuffer::fail_check_consume_literal ────────────────────

    #[tokio::test]
    async fn fail_check_consume_literal_no_fail_pattern() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let result = buf.fail_check_consume_literal("hello", None).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0, "hello");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_fail_pattern_not_matched() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let result = buf.fail_check_consume_literal("hello", Some(&fp)).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0, "hello");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_fail_pattern_triggers() {
        let buf = OutputBuffer::new();
        buf.append(b"ERROR: something broke").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let result = buf.fail_check_consume_literal("broke", Some(&fp)).await;
        let hit = result.unwrap_err();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
        // Buffer unchanged — fail pattern short-circuits before consume
        assert_eq!(buf.remaining().await, b"ERROR: something broke");
    }

    #[tokio::test]
    async fn fail_check_consume_literal_target_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let result = buf.fail_check_consume_literal("xyz", None).await;
        assert!(result.unwrap().is_none());
    }

    // ── OutputBuffer::fail_check_consume_regex ──────────────────────

    #[tokio::test]
    async fn fail_check_consume_regex_no_fail_pattern() {
        let buf = OutputBuffer::new();
        buf.append(b"abc 123 def").await;
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, None).await;
        let (m, _) = result.unwrap().unwrap();
        assert_eq!(m.value.0.get("0").unwrap(), "123");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_fail_pattern_triggers() {
        let buf = OutputBuffer::new();
        buf.append(b"FATAL: abc 123").await;
        let fp = FailPattern::Literal("FATAL".to_string());
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, Some(&fp)).await;
        let hit = result.unwrap_err();
        assert_eq!(hit.pattern, "FATAL");
        assert_eq!(hit.matched_text, "FATAL");
        // Buffer unchanged
        assert_eq!(buf.remaining().await, b"FATAL: abc 123");
    }

    #[tokio::test]
    async fn fail_check_consume_regex_target_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"hello world").await;
        let re = Regex::new(r"\d+").unwrap();
        let result = buf.fail_check_consume_regex(&re, None).await;
        assert!(result.unwrap().is_none());
    }

    // ── OutputBuffer::check_fail_pattern ─────────────────────────────

    #[tokio::test]
    async fn check_fail_pattern_none() {
        let buf = OutputBuffer::new();
        buf.append(b"ERROR here").await;
        assert!(buf.check_fail_pattern(None).await.is_none());
    }

    #[tokio::test]
    async fn check_fail_pattern_found() {
        let buf = OutputBuffer::new();
        buf.append(b"got ERROR output").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        let hit = buf.check_fail_pattern(Some(&fp)).await.unwrap();
        assert_eq!(hit.pattern, "ERROR");
        assert_eq!(hit.matched_text, "ERROR");
    }

    #[tokio::test]
    async fn check_fail_pattern_not_found() {
        let buf = OutputBuffer::new();
        buf.append(b"all good").await;
        let fp = FailPattern::Regex(Regex::new(r"ERROR").unwrap());
        assert!(buf.check_fail_pattern(Some(&fp)).await.is_none());
    }
}
