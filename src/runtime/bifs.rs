use async_trait::async_trait;

use crate::dsl::resolver::ir::Span;
use crate::runtime::progress::ProgressEvent;
use crate::runtime::result::Failure;
use crate::runtime::vm::Vm;

#[async_trait]
pub trait Bif: Send + Sync {
    fn name(&self) -> &str;
    fn arity(&self) -> usize;
    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &Span) -> Result<String, Failure>;
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

    async fn call(&self, vm: &mut Vm, args: Vec<String>, span: &Span) -> Result<String, Failure> {
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

    async fn call(&self, vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
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

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        // Placeholder: stores the message for future run-log integration.
        Ok(args[0].clone())
    }
}

// ─── Trim ──────────────────────────────────────────────────

pub struct Trim;

#[async_trait]
impl Bif for Trim {
    fn name(&self) -> &str { "trim" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].trim().to_string())
    }
}

// ─── Upper ─────────────────────────────────────────────────

pub struct Upper;

#[async_trait]
impl Bif for Upper {
    fn name(&self) -> &str { "upper" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_uppercase())
    }
}

// ─── Lower ─────────────────────────────────────────────────

pub struct Lower;

#[async_trait]
impl Bif for Lower {
    fn name(&self) -> &str { "lower" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].to_lowercase())
    }
}

// ─── Replace ───────────────────────────────────────────────

pub struct Replace;

#[async_trait]
impl Bif for Replace {
    fn name(&self) -> &str { "replace" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].replace(&args[1], &args[2]))
    }
}

// ─── Split ─────────────────────────────────────────────────

pub struct Split;

#[async_trait]
impl Bif for Split {
    fn name(&self) -> &str { "split" }
    fn arity(&self) -> usize { 3 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, span: &Span) -> Result<String, Failure> {
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

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(args[0].len().to_string())
    }
}

// ─── Uuid ──────────────────────────────────────────────────

pub struct Uuid;

#[async_trait]
impl Bif for Uuid {
    fn name(&self) -> &str { "uuid" }
    fn arity(&self) -> usize { 0 }

    async fn call(&self, _vm: &mut Vm, _args: Vec<String>, _span: &Span) -> Result<String, Failure> {
        Ok(uuid::Uuid::new_v4().to_string())
    }
}

// ─── Rand ──────────────────────────────────────────────────

pub struct Rand;

#[async_trait]
impl Bif for Rand {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 1 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, span: &Span) -> Result<String, Failure> {
        let n = parse_length(&args[0], span)?;
        Ok(random_string(n, ALPHANUM))
    }
}

pub struct RandWithMode;

#[async_trait]
impl Bif for RandWithMode {
    fn name(&self) -> &str { "rand" }
    fn arity(&self) -> usize { 2 }

    async fn call(&self, _vm: &mut Vm, args: Vec<String>, span: &Span) -> Result<String, Failure> {
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
    use rand::Rng;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| charset[rng.random_range(0..charset.len())] as char)
        .collect()
}
