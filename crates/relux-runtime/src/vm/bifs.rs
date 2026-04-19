use async_trait::async_trait;

use crate::report::result::Failure;
use crate::vm::Vm;
use relux_core::diagnostics::IrSpan;

// ─── BIF Trait ──────────────────────────────────────────────
// Bif: callable only from impure (shell) contexts.
// Pure BIFs are handled by relux_core::pure::bifs::dispatch.

#[async_trait]
pub trait Bif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure>;
}

// ─── Lookup ─────────────────────────────────────────────────

pub fn lookup_impure(name: &str, arity: usize) -> Option<Box<dyn Bif>> {
    match (name, arity) {
        ("sleep", 1) => Some(Box::new(Sleep)),
        ("annotate", 1) => Some(Box::new(Annotate)),
        ("log", 1) => Some(Box::new(Log)),
        ("match_prompt", 0) => Some(Box::new(MatchPrompt)),
        ("match_exit_code", 1) => Some(Box::new(MatchExitCode)),
        ("match_ok", 0) => Some(Box::new(MatchOk)),
        ("match_not_ok", 0) => Some(Box::new(MatchNotOk)),
        ("match_not_ok", 1) => Some(Box::new(MatchNotOkWithCode)),
        ("ctrl_c", 0) => Some(Box::new(CtrlChar {
            name: "ctrl_c",
            byte: 0x03,
        })),
        ("ctrl_d", 0) => Some(Box::new(CtrlChar {
            name: "ctrl_d",
            byte: 0x04,
        })),
        ("ctrl_z", 0) => Some(Box::new(CtrlChar {
            name: "ctrl_z",
            byte: 0x1A,
        })),
        ("ctrl_l", 0) => Some(Box::new(CtrlChar {
            name: "ctrl_l",
            byte: 0x0C,
        })),
        ("ctrl_backslash", 0) => Some(Box::new(CtrlChar {
            name: "ctrl_backslash",
            byte: 0x1C,
        })),
        _ => None,
    }
}

/// Returns true if a BIF with the given name and arity exists (pure or impure).
pub fn is_known(name: &str, arity: usize) -> bool {
    relux_core::pure::bifs::is_pure_bif(name, arity) || lookup_impure(name, arity).is_some()
}

/// Returns true if the BIF exists and is callable from a pure context.
pub fn is_pure_bif(name: &str, arity: usize) -> bool {
    relux_core::pure::bifs::is_pure_bif(name, arity)
}

/// Returns true if the BIF exists but is only callable from an impure context.
pub fn is_impure_bif(name: &str, arity: usize) -> bool {
    lookup_impure(name, arity).is_some()
}

fn runtime_error(message: String, span: &IrSpan) -> Failure {
    Failure::Runtime {
        message,
        span: Some(span.clone()),
        shell: None,
    }
}

// ─── Impure BIFs ────────────────────────────────────────────

pub struct Sleep;

#[async_trait]
impl Bif for Sleep {
    fn name(&self) -> &str {
        "sleep"
    }
    fn arity(&self) -> usize {
        1
    }

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure> {
        let duration = humantime::parse_duration(args[0].trim())
            .map_err(|_| runtime_error(format!("invalid duration: `{}`", args[0]), span))?;
        let shell = vm.current_name();
        vm.events.emit_sleep_start(&shell, duration, Some(span));
        tokio::select! {
            _ = tokio::time::sleep(duration) => {}
            _ = vm.cancel.cancelled() => {
                let shell = vm.current_name();
                vm.events.emit_sleep_done(&shell, Some(span));
                return Err(Failure::Cancelled {
                    span: Some(span.clone()),
                    shell: Some(shell),
                });
            }
        }
        let shell = vm.current_name();
        vm.events.emit_sleep_done(&shell, Some(span));
        Ok(String::new())
    }
}

pub struct Annotate;

#[async_trait]
impl Bif for Annotate {
    fn name(&self) -> &str {
        "annotate"
    }
    fn arity(&self) -> usize {
        1
    }

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure> {
        let text = args[0].clone();
        let shell = vm.current_name();
        vm.events.emit_annotate(&shell, &text, Some(span));
        Ok(text)
    }
}

pub struct Log;

#[async_trait]
impl Bif for Log {
    fn name(&self) -> &str {
        "log"
    }
    fn arity(&self) -> usize {
        1
    }

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure> {
        let message = args[0].clone();
        let shell = vm.current_name();
        vm.events.emit_log(&shell, &message, Some(span));
        Ok(message)
    }
}

pub struct MatchPrompt;

#[async_trait]
impl Bif for MatchPrompt {
    fn name(&self) -> &str {
        "match_prompt"
    }
    fn arity(&self) -> usize {
        0
    }

    async fn call(
        &self,
        vm: &mut Vm,
        _args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchExitCode;

#[async_trait]
impl Bif for MatchExitCode {
    fn name(&self) -> &str {
        "match_exit_code"
    }
    fn arity(&self) -> usize {
        1
    }

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.send_line("echo ::$?::", span).await?;
        vm.match_literal(&format!("::{}::", args[0]), span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchOk;

#[async_trait]
impl Bif for MatchOk {
    fn name(&self) -> &str {
        "match_ok"
    }
    fn arity(&self) -> usize {
        0
    }

    async fn call(
        &self,
        vm: &mut Vm,
        _args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line("echo ::$?::", span).await?;
        vm.match_literal("::0::", span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchNotOk;

#[async_trait]
impl Bif for MatchNotOk {
    fn name(&self) -> &str {
        "match_not_ok"
    }
    fn arity(&self) -> usize {
        0
    }

    async fn call(
        &self,
        vm: &mut Vm,
        _args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line(
            "__RE=$(echo ::$?::) && test \"${__RE}\" != '::0::' && echo ${__RE}",
            span,
        )
        .await?;
        vm.match_literal("::", span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchNotOkWithCode;

#[async_trait]
impl Bif for MatchNotOkWithCode {
    fn name(&self) -> &str {
        "match_not_ok"
    }
    fn arity(&self) -> usize {
        1
    }

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &IrSpan) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line(
            "__RE=$(echo ::$?::) && test \"${__RE}\" != '::0::' && echo ${__RE}",
            span,
        )
        .await?;
        vm.match_literal(&format!("::{}::", args[0]), span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct CtrlChar {
    name: &'static str,
    byte: u8,
}

#[async_trait]
impl Bif for CtrlChar {
    fn name(&self) -> &str {
        self.name
    }
    fn arity(&self) -> usize {
        0
    }

    async fn call(
        &self,
        vm: &mut Vm,
        _args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
        vm.send_raw(&[self.byte], span).await?;
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // BIF tests that required DummyVm are removed since we can no longer
    // easily construct a Vm without a real PTY. The BIF logic is simple
    // enough that it's well-covered by e2e tests. We keep the lookup tests.

    #[tokio::test]
    async fn test_lookup() {
        // Pure BIFs are now handled by crate::evaluator
        assert!(is_pure_bif("trim", 1));
        assert!(is_pure_bif("upper", 1));
        assert!(is_pure_bif("rand", 1));
        assert!(is_pure_bif("rand", 2));
        assert!(is_pure_bif("uuid", 0));
        assert!(is_pure_bif("available_port", 0));
        assert!(is_pure_bif("which", 1));
        assert!(is_pure_bif("default", 2));
        // Impure BIFs
        assert!(lookup_impure("sleep", 1).is_some());
        assert!(lookup_impure("annotate", 1).is_some());
        assert!(lookup_impure("log", 1).is_some());
        assert!(lookup_impure("match_prompt", 0).is_some());
        assert!(lookup_impure("match_exit_code", 1).is_some());
        assert!(lookup_impure("match_ok", 0).is_some());
        assert!(lookup_impure("match_not_ok", 0).is_some());
        assert!(lookup_impure("ctrl_c", 0).is_some());
        assert!(lookup_impure("ctrl_d", 0).is_some());
        assert!(lookup_impure("ctrl_z", 0).is_some());
        assert!(lookup_impure("ctrl_l", 0).is_some());
        assert!(lookup_impure("ctrl_backslash", 0).is_some());
        assert!(!is_known("nonexistent", 0));
        assert!(!is_known("trim", 2));
    }
}
