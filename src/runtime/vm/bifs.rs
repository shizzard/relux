use async_trait::async_trait;

use crate::diagnostics::IrSpan;
use crate::runtime::observe::progress::ProgressEvent;
use crate::runtime::report::result::Failure;

// ─── Context Trait ──────────────────────────────────────────
// VmContext: shell context for impure (shell-bound) BIFs.

#[async_trait]
pub trait VmContext: Send {
    fn emit_progress(&self, event: ProgressEvent);
    async fn emit_log(&mut self, message: String);
    async fn match_literal(&mut self, pattern: &str, span: &IrSpan) -> Result<String, Failure>;
    async fn send_line(&mut self, line: &str, span: &IrSpan) -> Result<(), Failure>;
    async fn send_raw(&mut self, data: &[u8], span: &IrSpan) -> Result<(), Failure>;
    fn shell_prompt(&self) -> &str;
}

// ─── BIF Trait ──────────────────────────────────────────────
// Bif: callable only from impure (shell) contexts.
// Pure BIFs are handled by crate::pure::bifs::dispatch.

#[async_trait]
pub trait Bif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure>;
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
    crate::pure::bifs::is_pure_bif(name, arity) || lookup_impure(name, arity).is_some()
}

/// Returns true if the BIF exists and is callable from a pure context.
pub fn is_pure_bif(name: &str, arity: usize) -> bool {
    crate::pure::bifs::is_pure_bif(name, arity)
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

    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
        let duration = humantime::parse_duration(args[0].trim())
            .map_err(|_| runtime_error(format!("invalid duration: `{}`", args[0]), span))?;
        vm.emit_progress(ProgressEvent::SleepStart);
        // TODO: select! with cancellation token to allow interrupting long sleeps
        tokio::time::sleep(duration).await;
        vm.emit_progress(ProgressEvent::SleepDone);
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

    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        _span: &IrSpan,
    ) -> Result<String, Failure> {
        let text = args[0].clone();
        vm.emit_progress(ProgressEvent::Annotation(text.clone()));
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

    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        _span: &IrSpan,
    ) -> Result<String, Failure> {
        let message = args[0].clone();
        vm.emit_log(message.clone()).await;
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
        vm: &mut dyn VmContext,
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

    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
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
        vm: &mut dyn VmContext,
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
        vm: &mut dyn VmContext,
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

    async fn call(
        &self,
        vm: &mut dyn VmContext,
        args: Vec<String>,
        span: &IrSpan,
    ) -> Result<String, Failure> {
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
        vm: &mut dyn VmContext,
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
    struct DummyVm;

    #[async_trait]
    impl VmContext for DummyVm {
        fn emit_progress(&self, _event: ProgressEvent) {}
        async fn emit_log(&mut self, _message: String) {}

        async fn match_literal(
            &mut self,
            pattern: &str,
            _span: &IrSpan,
        ) -> Result<String, Failure> {
            Ok(pattern.to_string())
        }

        async fn send_line(&mut self, _line: &str, _span: &IrSpan) -> Result<(), Failure> {
            Ok(())
        }

        async fn send_raw(&mut self, _data: &[u8], _span: &IrSpan) -> Result<(), Failure> {
            Ok(())
        }

        fn shell_prompt(&self) -> &str {
            "test> "
        }
    }

    fn dummy_span() -> IrSpan {
        IrSpan::synthetic()
    }

    #[tokio::test]
    async fn test_log() {
        let mut vm = DummyVm;
        let r = Log
            .call(&mut vm, vec!["a message".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "a message");
    }

    #[tokio::test]
    async fn test_annotate() {
        let mut vm = DummyVm;
        let r = Annotate
            .call(&mut vm, vec!["note".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "note");
    }

    #[tokio::test]
    async fn test_sleep_invalid_duration() {
        let mut vm = DummyVm;
        let r = Sleep
            .call(&mut vm, vec!["not-a-duration".into()], &dummy_span())
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn test_match_prompt() {
        let mut vm = DummyVm;
        let r = MatchPrompt
            .call(&mut vm, vec![], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_exit_code() {
        let mut vm = DummyVm;
        let r = MatchExitCode
            .call(&mut vm, vec!["0".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_exit_code_non_numeric() {
        let mut vm = DummyVm;
        let r = MatchExitCode
            .call(&mut vm, vec!["abc".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_ok() {
        let mut vm = DummyVm;
        let r = MatchOk.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_not_ok() {
        let mut vm = DummyVm;
        let r = MatchNotOk
            .call(&mut vm, vec![], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_not_ok_with_code() {
        let mut vm = DummyVm;
        let r = MatchNotOkWithCode
            .call(&mut vm, vec!["2".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_ctrl_c() {
        let mut vm = DummyVm;
        let r = CtrlChar {
            name: "ctrl_c",
            byte: 0x03,
        }
        .call(&mut vm, vec![], &dummy_span())
        .await
        .unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_d() {
        let mut vm = DummyVm;
        let r = CtrlChar {
            name: "ctrl_d",
            byte: 0x04,
        }
        .call(&mut vm, vec![], &dummy_span())
        .await
        .unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_z() {
        let mut vm = DummyVm;
        let r = CtrlChar {
            name: "ctrl_z",
            byte: 0x1A,
        }
        .call(&mut vm, vec![], &dummy_span())
        .await
        .unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_l() {
        let mut vm = DummyVm;
        let r = CtrlChar {
            name: "ctrl_l",
            byte: 0x0C,
        }
        .call(&mut vm, vec![], &dummy_span())
        .await
        .unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_backslash() {
        let mut vm = DummyVm;
        let r = CtrlChar {
            name: "ctrl_backslash",
            byte: 0x1C,
        }
        .call(&mut vm, vec![], &dummy_span())
        .await
        .unwrap();
        assert_eq!(r, "");
    }

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
