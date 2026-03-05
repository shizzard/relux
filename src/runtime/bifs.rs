use async_trait::async_trait;

use crate::dsl::resolver::ir::Span;
use crate::runtime::progress::ProgressEvent;
use crate::runtime::result::Failure;

#[async_trait]
pub trait VmContext: Send {
    fn emit_progress(&self, event: ProgressEvent);
    async fn match_literal(&mut self, pattern: &str, span: &Span) -> Result<String, Failure>;
    async fn send_line(&mut self, line: &str, span: &Span) -> Result<(), Failure>;
    fn shell_prompt(&self) -> &str;
}

#[async_trait]
pub trait Bif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure>;
}

pub fn lookup(name: &str, arity: usize) -> Option<Box<dyn Bif>> {
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
        ("match_prompt", 0) => Some(Box::new(MatchPrompt)),
        ("match_exit_code", 1) => Some(Box::new(MatchExitCode)),
        ("match_ok", 0) => Some(Box::new(MatchOk)),
        _ => None,
    }
}

fn runtime_error(message: String, span: &Span) -> Failure {
    Failure::Runtime {
        message,
        span: Some(span.clone()),
        shell: None,
    }
}

// ─── Sleep ─────────────────────────────────────────────────

pub struct Sleep;

#[async_trait]
impl Bif for Sleep {
    fn name(&self) -> &str { "sleep" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let duration = humantime::parse_duration(args[0].trim())
            .map_err(|_| runtime_error(format!("invalid duration: `{}`", args[0]), span))?;
        vm.emit_progress(ProgressEvent::SleepStart);
        tokio::time::sleep(duration).await;
        vm.emit_progress(ProgressEvent::SleepDone);
        Ok(String::new())
    }
}

// ─── Annotate ──────────────────────────────────────────────

pub struct Annotate;

#[async_trait]
impl Bif for Annotate {
    fn name(&self) -> &str { "annotate" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        let text = args[0].clone();
        vm.emit_progress(ProgressEvent::Annotation(text.clone()));
        Ok(text)
    }
}

// ─── Log ───────────────────────────────────────────────────

pub struct Log;

#[async_trait]
impl Bif for Log {
    fn name(&self) -> &str { "log" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].clone())
    }
}

// ─── Trim ──────────────────────────────────────────────────

pub struct Trim;

#[async_trait]
impl Bif for Trim {
    fn name(&self) -> &str { "trim" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].trim().to_string())
    }
}

// ─── Upper ─────────────────────────────────────────────────

pub struct Upper;

#[async_trait]
impl Bif for Upper {
    fn name(&self) -> &str { "upper" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_uppercase())
    }
}

// ─── Lower ─────────────────────────────────────────────────

pub struct Lower;

#[async_trait]
impl Bif for Lower {
    fn name(&self) -> &str { "lower" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_lowercase())
    }
}

// ─── Replace ───────────────────────────────────────────────

pub struct Replace;

#[async_trait]
impl Bif for Replace {
    fn name(&self) -> &str { "replace" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].replace(&args[1], &args[2]))
    }
}

// ─── Split ─────────────────────────────────────────────────

pub struct Split;

#[async_trait]
impl Bif for Split {
    fn name(&self) -> &str { "split" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let index: usize = args[2].parse()
            .map_err(|_| runtime_error(format!("invalid index: `{}`", args[2]), span))?;
        let parts: Vec<&str> = args[0].split(&args[1]).collect();
        Ok(parts.get(index).unwrap_or(&"").to_string())
    }
}

// ─── Len ───────────────────────────────────────────────────

pub struct Len;

#[async_trait]
impl Bif for Len {
    fn name(&self) -> &str { "len" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].len().to_string())
    }
}

// ─── Uuid ──────────────────────────────────────────────────

pub struct Uuid;

#[async_trait]
impl Bif for Uuid {
    fn name(&self) -> &str { "uuid" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, _vm: &mut dyn VmContext, _args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(uuid::Uuid::new_v4().to_string())
    }
}

// ─── Rand ──────────────────────────────────────────────────

pub struct Rand;

#[async_trait]
impl Bif for Rand {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let n = parse_length(&args[0], span)?;
        Ok(random_string(n, ALPHANUM))
    }
}

pub struct RandWithMode;

#[async_trait]
impl Bif for RandWithMode {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 2 }

    async fn call(&self, _vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
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

// ─── MatchPrompt ────────────────────────────────────────────

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

// ─── MatchExitCode ──────────────────────────────────────────

pub struct MatchExitCode;

#[async_trait]
impl Bif for MatchExitCode {
    fn name(&self) -> &str { "match_exit_code" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, vm: &mut dyn VmContext, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.send_line("echo $?", span).await?;
        vm.match_literal(&args[0], span).await?;
        vm.match_literal(&prompt, span).await
    }
}

// ─── MatchOk ────────────────────────────────────────────────

pub struct MatchOk;

#[async_trait]
impl Bif for MatchOk {
    fn name(&self) -> &str { "match_ok" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, vm: &mut dyn VmContext, _args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let prompt = vm.shell_prompt().to_string();
        vm.match_literal(&prompt, span).await?;
        vm.send_line("echo $?", span).await?;
        vm.match_literal("0", span).await?;
        vm.match_literal(&prompt, span).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyVm;

    #[async_trait]
    impl VmContext for DummyVm {
        fn emit_progress(&self, _event: ProgressEvent) {}

        async fn match_literal(&mut self, pattern: &str, _span: &Span) -> Result<String, Failure> {
            Ok(pattern.to_string())
        }

        async fn send_line(&mut self, _line: &str, _span: &Span) -> Result<(), Failure> {
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
    async fn test_lookup() {
        assert!(lookup("trim", 1).is_some());
        assert!(lookup("upper", 1).is_some());
        assert!(lookup("rand", 1).is_some());
        assert!(lookup("rand", 2).is_some());
        assert!(lookup("uuid", 0).is_some());
        assert!(lookup("match_prompt", 0).is_some());
        assert!(lookup("match_exit_code", 1).is_some());
        assert!(lookup("match_ok", 0).is_some());
        assert!(lookup("nonexistent", 0).is_none());
        assert!(lookup("trim", 2).is_none());
    }
}
