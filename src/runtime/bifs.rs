use async_trait::async_trait;

use crate::dsl::resolver::ir::Span;
use crate::runtime::progress::ProgressEvent;
use crate::runtime::result::Failure;

// ─── Context Traits ─────────────────────────────────────────
// PureContext: available everywhere (pure + impure functions).
// VmContext: shell context, extends PureContext (impure only).

#[async_trait]
pub trait PureContext: Send {
    fn emit_progress(&self, event: ProgressEvent);
    async fn emit_log(&mut self, message: String);
}

#[async_trait]
pub trait VmContext: PureContext {
    async fn match_literal(&mut self, pattern: &str, span: &Span) -> Result<String, Failure>;
    async fn send_line(&mut self, line: &str, span: &Span) -> Result<(), Failure>;
    async fn send_raw(&mut self, data: &[u8], span: &Span) -> Result<(), Failure>;
    fn shell_prompt(&self) -> &str;
}

// ─── BIF Traits ─────────────────────────────────────────────
// PureBif: callable from pure and impure contexts.
// Bif: callable only from impure (shell) contexts.

#[async_trait]
pub trait PureBif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, ctx: &mut dyn PureContext, args: Vec<String>, span: &Span) -> Result<String, Failure>;
}

#[async_trait]
pub trait Bif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure>;
}

// ─── Lookup ─────────────────────────────────────────────────

pub fn lookup_pure(name: &str, arity: usize) -> Option<Box<dyn PureBif>> {
    match (name, arity) {
        ("sleep", 1) => Some(Box::new(Sleep)),
        ("annotate", 1) => Some(Box::new(Annotate)),
        ("log", 1) => Some(Box::new(Log)),
        ("trim", 1) => Some(Box::new(Trim)),
        ("upper", 1) => Some(Box::new(Upper)),
        ("lower", 1) => Some(Box::new(Lower)),
        ("replace", 3) => Some(Box::new(Replace)),
        ("split", 3) => Some(Box::new(Split)),
        ("len", 1) => Some(Box::new(Len)),
        ("uuid", 0) => Some(Box::new(Uuid)),
        ("rand", 1) => Some(Box::new(Rand)),
        ("rand", 2) => Some(Box::new(RandWithMode)),
        ("available_port", 0) => Some(Box::new(AvailablePort)),
        ("which", 1) => Some(Box::new(Which)),
        _ => None,
    }
}

pub fn lookup_impure(name: &str, arity: usize) -> Option<Box<dyn Bif>> {
    match (name, arity) {
        ("match_prompt", 0) => Some(Box::new(MatchPrompt)),
        ("match_exit_code", 1) => Some(Box::new(MatchExitCode)),
        ("match_ok", 0) => Some(Box::new(MatchOk)),
        ("match_not_ok", 0) => Some(Box::new(MatchNotOk)),
        ("match_not_ok", 1) => Some(Box::new(MatchNotOkWithCode)),
        ("ctrl_c", 0) => Some(Box::new(CtrlChar { name: "ctrl_c", byte: 0x03 })),
        ("ctrl_d", 0) => Some(Box::new(CtrlChar { name: "ctrl_d", byte: 0x04 })),
        ("ctrl_z", 0) => Some(Box::new(CtrlChar { name: "ctrl_z", byte: 0x1A })),
        ("ctrl_l", 0) => Some(Box::new(CtrlChar { name: "ctrl_l", byte: 0x0C })),
        ("ctrl_backslash", 0) => Some(Box::new(CtrlChar { name: "ctrl_backslash", byte: 0x1C })),
        _ => None,
    }
}

/// Returns true if a BIF with the given name and arity exists (pure or impure).
pub fn is_known(name: &str, arity: usize) -> bool {
    lookup_pure(name, arity).is_some() || lookup_impure(name, arity).is_some()
}

fn runtime_error(message: String, span: &Span) -> Failure {
    Failure::Runtime {
        message,
        span: Some(span.clone()),
        shell: None,
    }
}

// ─── Pure BIFs ──────────────────────────────────────────────

pub struct Sleep;

#[async_trait]
impl PureBif for Sleep {
    fn name(&self) -> &str { "sleep" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, ctx: &mut dyn PureContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let duration = humantime::parse_duration(args[0].trim())
            .map_err(|_| runtime_error(format!("invalid duration: `{}`", args[0]), span))?;
        ctx.emit_progress(ProgressEvent::SleepStart);
        tokio::time::sleep(duration).await;
        ctx.emit_progress(ProgressEvent::SleepDone);
        Ok(String::new())
    }
}

pub struct Annotate;

#[async_trait]
impl PureBif for Annotate {
    fn name(&self) -> &str { "annotate" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        let text = args[0].clone();
        ctx.emit_progress(ProgressEvent::Annotation(text.clone()));
        Ok(text)
    }
}

pub struct Log;

#[async_trait]
impl PureBif for Log {
    fn name(&self) -> &str { "log" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        let message = args[0].clone();
        ctx.emit_log(message.clone()).await;
        Ok(message)
    }
}

pub struct Trim;

#[async_trait]
impl PureBif for Trim {
    fn name(&self) -> &str { "trim" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].trim().to_string())
    }
}

pub struct Upper;

#[async_trait]
impl PureBif for Upper {
    fn name(&self) -> &str { "upper" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_uppercase())
    }
}

pub struct Lower;

#[async_trait]
impl PureBif for Lower {
    fn name(&self) -> &str { "lower" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_lowercase())
    }
}

pub struct Replace;

#[async_trait]
impl PureBif for Replace {
    fn name(&self) -> &str { "replace" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].replace(&args[1], &args[2]))
    }
}

pub struct Split;

#[async_trait]
impl PureBif for Split {
    fn name(&self) -> &str { "split" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let index: usize = args[2].parse()
            .map_err(|_| runtime_error(format!("invalid index: `{}`", args[2]), span))?;
        let parts: Vec<&str> = args[0].split(&args[1]).collect();
        Ok(parts.get(index).unwrap_or(&"").to_string())
    }
}

pub struct Len;

#[async_trait]
impl PureBif for Len {
    fn name(&self) -> &str { "len" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].len().to_string())
    }
}

pub struct Uuid;

#[async_trait]
impl PureBif for Uuid {
    fn name(&self) -> &str { "uuid" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, _ctx: &mut dyn PureContext, _args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(uuid::Uuid::new_v4().to_string())
    }
}

pub struct Rand;

#[async_trait]
impl PureBif for Rand {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let n = parse_length(&args[0], span)?;
        Ok(random_string(n, ALPHANUM))
    }
}

pub struct RandWithMode;

#[async_trait]
impl PureBif for RandWithMode {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 2 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let n = parse_length(&args[0], span)?;
        let charset = match args[1].as_str() {
            "alpha" => ALPHA,
            "num" => NUM,
            "alphanum" => ALPHANUM,
            "hex" => HEX,
            "oct" => OCT,
            "bin" => BIN,
            other => return Err(runtime_error(
                format!("unknown rand mode: `{other}` (expected: alpha, num, alphanum, hex, oct, bin)"),
                span,
            )),
        };
        Ok(random_string(n, charset))
    }
}

const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const NUM: &[u8] = b"0123456789";
const ALPHANUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const HEX: &[u8] = b"0123456789abcdef";
const OCT: &[u8] = b"01234567";
const BIN: &[u8] = b"01";

fn parse_length(s: &str, span: &Span) -> Result<usize, Failure> {
    s.parse()
        .map_err(|_| runtime_error(format!("invalid length: `{s}`"), span))
}

fn random_string(len: usize, charset: &[u8]) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| charset[rng.random_range(0..charset.len())] as char)
        .collect()
}

pub struct AvailablePort;

#[async_trait]
impl PureBif for AvailablePort {
    fn name(&self) -> &str { "available_port" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, _ctx: &mut dyn PureContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| runtime_error(format!("failed to bind to ephemeral port: {e}"), span))?;
        let port = listener.local_addr()
            .map_err(|e| runtime_error(format!("failed to get local address: {e}"), span))?
            .port();
        Ok(port.to_string())
    }
}

pub struct Which;

#[async_trait]
impl PureBif for Which {
    fn name(&self) -> &str { "which" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _ctx: &mut dyn PureContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        let name = &args[0];
        if name.is_empty() {
            return Ok(String::new());
        }

        // If the name contains a path separator, check it directly
        if name.contains(std::path::MAIN_SEPARATOR) {
            let path = std::path::Path::new(name);
            if is_executable(path) {
                return Ok(path.to_string_lossy().into_owned());
            }
            return Ok(String::new());
        }

        let path_var = std::env::var("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
        Ok(String::new())
    }
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

// ─── Impure BIFs ────────────────────────────────────────────

pub struct MatchPrompt;

#[async_trait]
impl Bif for MatchPrompt {
    fn name(&self) -> &str { "match_prompt" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, vm: &mut dyn VmContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchExitCode;

#[async_trait]
impl Bif for MatchExitCode {
    fn name(&self) -> &str { "match_exit_code" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.send_line("echo ::$?::", span).await?;
        vm.match_literal(&format!("::{}::", args[0]), span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchOk;

#[async_trait]
impl Bif for MatchOk {
    fn name(&self) -> &str { "match_ok" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, vm: &mut dyn VmContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
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
    fn name(&self) -> &str { "match_not_ok" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, vm: &mut dyn VmContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line("__RE=$(echo ::$?::) && test \"${__RE}\" != '::0::' && echo ${__RE}", span).await?;
        vm.match_literal("::", span).await?;
        vm.match_literal(&prompt, span).await
    }
}

pub struct MatchNotOkWithCode;

#[async_trait]
impl Bif for MatchNotOkWithCode {
    fn name(&self) -> &str { "match_not_ok" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line(
            "__RE=$(echo ::$?::) && test \"${__RE}\" != '::0::' && echo ${__RE}",
            span,
        ).await?;
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
    fn name(&self) -> &str { self.name }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, vm: &mut dyn VmContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
        vm.send_raw(&[self.byte], span).await?;
        Ok(String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyVm;

    #[async_trait]
    impl PureContext for DummyVm {
        fn emit_progress(&self, _event: ProgressEvent) {}
        async fn emit_log(&mut self, _message: String) {}
    }

    #[async_trait]
    impl VmContext for DummyVm {
        async fn match_literal(&mut self, pattern: &str, _span: &Span) -> Result<String, Failure> {
            Ok(pattern.to_string())
        }

        async fn send_line(&mut self, _line: &str, _span: &Span) -> Result<(), Failure> {
            Ok(())
        }

        async fn send_raw(&mut self, _data: &[u8], _span: &Span) -> Result<(), Failure> {
            Ok(())
        }

        fn shell_prompt(&self) -> &str {
            "test> "
        }
    }

    fn dummy_span() -> Span {
        Span::new(0, 0..0)
    }

    #[tokio::test]
    async fn test_trim() {
        let mut vm = DummyVm;
        let r = Trim.call(&mut vm, vec!["  hello  ".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "hello");
    }

    #[tokio::test]
    async fn test_upper() {
        let mut vm = DummyVm;
        let r = Upper.call(&mut vm, vec!["hello".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "HELLO");
    }

    #[tokio::test]
    async fn test_lower() {
        let mut vm = DummyVm;
        let r = Lower.call(&mut vm, vec!["HELLO".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "hello");
    }

    #[tokio::test]
    async fn test_replace() {
        let mut vm = DummyVm;
        let r = Replace
            .call(&mut vm, vec!["hello world".into(), "world".into(), "relux".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "hello relux");
    }

    #[tokio::test]
    async fn test_split() {
        let mut vm = DummyVm;
        let r = Split
            .call(&mut vm, vec!["a,b,c".into(), ",".into(), "1".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "b");
    }

    #[tokio::test]
    async fn test_split_out_of_bounds() {
        let mut vm = DummyVm;
        let r = Split
            .call(&mut vm, vec!["a,b".into(), ",".into(), "5".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_split_invalid_index() {
        let mut vm = DummyVm;
        let r = Split
            .call(&mut vm, vec!["a,b".into(), ",".into(), "xyz".into()], &dummy_span())
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn test_len() {
        let mut vm = DummyVm;
        let r = Len.call(&mut vm, vec!["hello".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "5");
    }

    #[tokio::test]
    async fn test_uuid() {
        let mut vm = DummyVm;
        let r = Uuid.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r.len(), 36);
        assert!(uuid::Uuid::parse_str(&r).is_ok());
    }

    #[tokio::test]
    async fn test_rand_default() {
        let mut vm = DummyVm;
        let r = Rand.call(&mut vm, vec!["8".into()], &dummy_span()).await.unwrap();
        assert_eq!(r.len(), 8);
        assert!(r.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[tokio::test]
    async fn test_rand_hex() {
        let mut vm = DummyVm;
        let r = RandWithMode
            .call(&mut vm, vec!["6".into(), "hex".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r.len(), 6);
        assert!(r.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn test_rand_bin() {
        let mut vm = DummyVm;
        let r = RandWithMode
            .call(&mut vm, vec!["8".into(), "bin".into()], &dummy_span())
            .await
            .unwrap();
        assert_eq!(r.len(), 8);
        assert!(r.chars().all(|c| c == '0' || c == '1'));
    }

    #[tokio::test]
    async fn test_rand_invalid_mode() {
        let mut vm = DummyVm;
        let r = RandWithMode
            .call(&mut vm, vec!["4".into(), "nope".into()], &dummy_span())
            .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn test_rand_invalid_length() {
        let mut vm = DummyVm;
        let r = Rand.call(&mut vm, vec!["abc".into()], &dummy_span()).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn test_log() {
        let mut vm = DummyVm;
        let r = Log.call(&mut vm, vec!["a message".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "a message");
    }

    #[tokio::test]
    async fn test_annotate() {
        let mut vm = DummyVm;
        let r = Annotate.call(&mut vm, vec!["note".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "note");
    }

    #[tokio::test]
    async fn test_sleep_invalid_duration() {
        let mut vm = DummyVm;
        let r = Sleep.call(&mut vm, vec!["not-a-duration".into()], &dummy_span()).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn test_match_prompt() {
        let mut vm = DummyVm;
        let r = MatchPrompt.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_exit_code() {
        let mut vm = DummyVm;
        let r = MatchExitCode.call(&mut vm, vec!["0".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_exit_code_non_numeric() {
        let mut vm = DummyVm;
        let r = MatchExitCode.call(&mut vm, vec!["abc".into()], &dummy_span()).await.unwrap();
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
        let r = MatchNotOk.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_match_not_ok_with_code() {
        let mut vm = DummyVm;
        let r = MatchNotOkWithCode.call(&mut vm, vec!["2".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "test> ");
    }

    #[tokio::test]
    async fn test_ctrl_c() {
        let mut vm = DummyVm;
        let r = CtrlChar { name: "ctrl_c", byte: 0x03 }.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_d() {
        let mut vm = DummyVm;
        let r = CtrlChar { name: "ctrl_d", byte: 0x04 }.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_z() {
        let mut vm = DummyVm;
        let r = CtrlChar { name: "ctrl_z", byte: 0x1A }.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_l() {
        let mut vm = DummyVm;
        let r = CtrlChar { name: "ctrl_l", byte: 0x0C }.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_ctrl_backslash() {
        let mut vm = DummyVm;
        let r = CtrlChar { name: "ctrl_backslash", byte: 0x1C }.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_available_port() {
        let mut vm = DummyVm;
        let r = AvailablePort.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        let port: u16 = r.parse().expect("should be a valid port number");
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_available_port_unique() {
        let mut vm = DummyVm;
        let a = AvailablePort.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        let b = AvailablePort.call(&mut vm, vec![], &dummy_span()).await.unwrap();
        // Not guaranteed but extremely likely with ephemeral ports
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn test_which_finds_sh() {
        let mut vm = DummyVm;
        let r = Which.call(&mut vm, vec!["sh".into()], &dummy_span()).await.unwrap();
        assert!(!r.is_empty(), "which(\"sh\") should find sh on PATH");
        assert!(r.ends_with("/sh"), "result should be an absolute path ending in /sh, got: {r}");
    }

    #[tokio::test]
    async fn test_which_not_found() {
        let mut vm = DummyVm;
        let r = Which.call(&mut vm, vec!["nonexistent_program_xyz_123".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_which_empty_name() {
        let mut vm = DummyVm;
        let r = Which.call(&mut vm, vec!["".into()], &dummy_span()).await.unwrap();
        assert_eq!(r, "");
    }

    #[tokio::test]
    async fn test_which_result_is_executable() {
        let mut vm = DummyVm;
        let r = Which.call(&mut vm, vec!["sh".into()], &dummy_span()).await.unwrap();
        assert!(!r.is_empty());
        let metadata = std::fs::metadata(&r).expect("path should exist");
        use std::os::unix::fs::PermissionsExt;
        assert!(metadata.permissions().mode() & 0o111 != 0, "result should be executable");
    }

    #[tokio::test]
    async fn test_lookup() {
        assert!(lookup_pure("available_port", 0).is_some());
        assert!(lookup_pure("trim", 1).is_some());
        assert!(lookup_pure("upper", 1).is_some());
        assert!(lookup_pure("rand", 1).is_some());
        assert!(lookup_pure("rand", 2).is_some());
        assert!(lookup_pure("uuid", 0).is_some());
        assert!(lookup_pure("which", 1).is_some());
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
