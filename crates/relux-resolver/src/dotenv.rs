//! dotenv line parser.
//!
//! Vendored and adapted from dotenvy 0.15.7 (`src/parse.rs` and the
//! `QuotedLines` multiline assembler from `src/iter.rs`), MIT-licensed,
//! Copyright (c) 2015 Noemi Lapresta and dotenvy contributors. Changes from
//! upstream:
//!   * `${VAR}` substitution resolves through a `LayeredEnvBuilder`
//!     (same-file `own` -> ancestor stack -> `Base`) instead of `std::env`;
//!     the `std::env::var` lookup in `apply_substitution` is removed.
//!   * Errors are expressed with `thiserror` (upstream's `EnvVar` variant,
//!     now dead, is dropped).
//!   * The environment-mutating `Iter::load` paths are omitted; parsing never
//!     touches the process environment.

use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;

use relux_core::pure::LayeredEnvBuilder;
use thiserror::Error;

/// Failure parsing a `.env` reader.
#[derive(Debug, Error)]
pub enum DotenvParseError {
    /// A line could not be parsed; carries the offending line and byte column.
    #[error("malformed .env line (column {1}): {0}")]
    LineParse(String, usize),
    /// I/O failure while reading the `.env` stream (includes non-UTF-8 input).
    #[error("i/o error reading .env")]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, DotenvParseError>;

/// Parse a `.env` reader into ordered `(key, value)` pairs, resolving `${VAR}`
/// through `builder`. Each parsed line is also recorded into `builder` so later
/// lines in the same file can reference earlier ones. Never touches `std::env`.
pub fn parse_env<R: Read>(
    reader: R,
    builder: &mut LayeredEnvBuilder,
) -> Result<Vec<(String, String)>> {
    let mut lines = QuotedLines {
        buf: BufReader::new(reader),
    };
    remove_bom(&mut lines.buf)?;
    let mut out = Vec::new();
    for line in lines {
        let line = line?;
        if let Some(pair) = parse_line(&line, builder)? {
            out.push(pair);
        }
    }
    Ok(out)
}

fn remove_bom<B: BufRead>(buf: &mut B) -> Result<()> {
    // https://www.compart.com/en/unicode/U+FEFF
    if buf.fill_buf()?.starts_with(&[0xEF, 0xBB, 0xBF]) {
        buf.consume(3);
    }
    Ok(())
}

fn apply_substitution(builder: &LayeredEnvBuilder, name: &str, output: &mut String) {
    // Upstream dotenvy consulted std::env::var here first; removed so that
    // substitution resolves only through the builder (same-file own -> ancestor
    // stack -> Base). Unbound names expand to the empty string.
    output.push_str(builder.get(name).unwrap_or_default());
}

// --- vendored from dotenvy 0.15.7 src/parse.rs (LineParser, SubstitutionMode,
// parse_value), with the substitution store retyped to `LayeredEnvBuilder` and
// errors retyped to `DotenvParseError` ---

type ParsedLine = Result<Option<(String, String)>>;

fn parse_line(line: &str, builder: &mut LayeredEnvBuilder) -> ParsedLine {
    let mut parser = LineParser::new(line, builder);
    parser.parse_line()
}

struct LineParser<'a> {
    original_line: &'a str,
    builder: &'a mut LayeredEnvBuilder,
    line: &'a str,
    pos: usize,
}

impl<'a> LineParser<'a> {
    fn new(line: &'a str, builder: &'a mut LayeredEnvBuilder) -> LineParser<'a> {
        LineParser {
            original_line: line,
            builder,
            line: line.trim_end(), // we don't want trailing whitespace
            pos: 0,
        }
    }

    fn err(&self) -> DotenvParseError {
        DotenvParseError::LineParse(self.original_line.into(), self.pos)
    }

    fn parse_line(&mut self) -> ParsedLine {
        self.skip_whitespace();
        // if its an empty line or a comment, skip it
        if self.line.is_empty() || self.line.starts_with('#') {
            return Ok(None);
        }

        let mut key = self.parse_key()?;
        self.skip_whitespace();

        // export can be either an optional prefix or a key itself
        if key == "export" {
            // here we check for an optional `=`, below we throw directly when it's not found.
            if self.expect_equal().is_err() {
                key = self.parse_key()?;
                self.skip_whitespace();
                self.expect_equal()?;
            }
        } else {
            self.expect_equal()?;
        }
        self.skip_whitespace();

        if self.line.is_empty() || self.line.starts_with('#') {
            self.builder.insert(key.clone(), String::new());
            return Ok(Some((key, String::new())));
        }

        let parsed_value = parse_value(self.line, self.builder)?;
        self.builder.insert(key.clone(), parsed_value.clone());

        Ok(Some((key, parsed_value)))
    }

    fn parse_key(&mut self) -> Result<String> {
        if !self
            .line
            .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        {
            return Err(self.err());
        }
        let index = match self
            .line
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        {
            Some(index) => index,
            None => self.line.len(),
        };
        self.pos += index;
        let key = String::from(&self.line[..index]);
        self.line = &self.line[index..];
        Ok(key)
    }

    fn expect_equal(&mut self) -> Result<()> {
        if !self.line.starts_with('=') {
            return Err(self.err());
        }
        self.line = &self.line[1..];
        self.pos += 1;
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        if let Some(index) = self.line.find(|c: char| !c.is_whitespace()) {
            self.pos += index;
            self.line = &self.line[index..];
        } else {
            self.pos += self.line.len();
            self.line = "";
        }
    }
}

#[derive(Eq, PartialEq)]
enum SubstitutionMode {
    None,
    Block,
    EscapedBlock,
}

// Vendored from dotenvy 0.15.7; the `.drain(..).collect()` calls below are
// upstream's substitution-name reset idiom, kept as-is rather than
// restructured to `mem::take` to preserve a faithful copy of the parser.
#[allow(clippy::drain_collect)]
fn parse_value(input: &str, builder: &LayeredEnvBuilder) -> Result<String> {
    let mut strong_quote = false; // '
    let mut weak_quote = false; // "
    let mut escaped = false;
    let mut expecting_end = false;

    //FIXME can this be done without yet another allocation per line?
    let mut output = String::new();

    let mut substitution_mode = SubstitutionMode::None;
    let mut substitution_name = String::new();

    for (index, c) in input.chars().enumerate() {
        //the regex _should_ already trim whitespace off the end
        //expecting_end is meant to permit: k=v #comment
        //without affecting: k=v#comment
        //and throwing on: k=v w
        if expecting_end {
            if c == ' ' || c == '\t' {
                continue;
            } else if c == '#' {
                break;
            } else {
                return Err(DotenvParseError::LineParse(input.to_owned(), index));
            }
        } else if escaped {
            //TODO I tried handling literal \r but various issues
            //imo not worth worrying about until there's a use case
            //(actually handling backslash 0x10 would be a whole other matter)
            //then there's \v \f bell hex... etc
            match c {
                '\\' | '\'' | '"' | '$' | ' ' => output.push(c),
                'n' => output.push('\n'), // handle \n case
                _ => {
                    return Err(DotenvParseError::LineParse(input.to_owned(), index));
                }
            }

            escaped = false;
        } else if strong_quote {
            if c == '\'' {
                strong_quote = false;
            } else {
                output.push(c);
            }
        } else if substitution_mode != SubstitutionMode::None {
            if c.is_alphanumeric() {
                substitution_name.push(c);
            } else {
                match substitution_mode {
                    SubstitutionMode::None => unreachable!(),
                    SubstitutionMode::Block => {
                        if c == '{' && substitution_name.is_empty() {
                            substitution_mode = SubstitutionMode::EscapedBlock;
                        } else {
                            apply_substitution(
                                builder,
                                &substitution_name.drain(..).collect::<String>(),
                                &mut output,
                            );
                            if c == '$' {
                                substitution_mode = if !strong_quote && !escaped {
                                    SubstitutionMode::Block
                                } else {
                                    SubstitutionMode::None
                                }
                            } else {
                                substitution_mode = SubstitutionMode::None;
                                output.push(c);
                            }
                        }
                    }
                    SubstitutionMode::EscapedBlock => {
                        if c == '}' {
                            substitution_mode = SubstitutionMode::None;
                            apply_substitution(
                                builder,
                                &substitution_name.drain(..).collect::<String>(),
                                &mut output,
                            );
                        } else {
                            substitution_name.push(c);
                        }
                    }
                }
            }
        } else if c == '$' {
            substitution_mode = if !strong_quote && !escaped {
                SubstitutionMode::Block
            } else {
                SubstitutionMode::None
            }
        } else if weak_quote {
            if c == '"' {
                weak_quote = false;
            } else if c == '\\' {
                escaped = true;
            } else {
                output.push(c);
            }
        } else if c == '\'' {
            strong_quote = true;
        } else if c == '"' {
            weak_quote = true;
        } else if c == '\\' {
            escaped = true;
        } else if c == ' ' || c == '\t' {
            expecting_end = true;
        } else {
            output.push(c);
        }
    }

    //XXX also fail if escaped? or...
    if substitution_mode == SubstitutionMode::EscapedBlock || strong_quote || weak_quote {
        let value_length = input.len();
        Err(DotenvParseError::LineParse(
            input.to_owned(),
            if value_length == 0 {
                0
            } else {
                value_length - 1
            },
        ))
    } else {
        apply_substitution(
            builder,
            &substitution_name.drain(..).collect::<String>(),
            &mut output,
        );
        Ok(output)
    }
}

// --- vendored from dotenvy 0.15.7 src/iter.rs (QuotedLines, ParseState,
// eval_end_state, Iterator for QuotedLines), with errors retyped to
// `DotenvParseError` ---

struct QuotedLines<B> {
    buf: B,
}

enum ParseState {
    Complete,
    Escape,
    StrongOpen,
    StrongOpenEscape,
    WeakOpen,
    WeakOpenEscape,
    Comment,
    WhiteSpace,
}

fn eval_end_state(prev_state: ParseState, buf: &str) -> (usize, ParseState) {
    let mut cur_state = prev_state;
    let mut cur_pos: usize = 0;

    for (pos, c) in buf.char_indices() {
        cur_pos = pos;
        cur_state = match cur_state {
            ParseState::WhiteSpace => match c {
                '#' => return (cur_pos, ParseState::Comment),
                '\\' => ParseState::Escape,
                '"' => ParseState::WeakOpen,
                '\'' => ParseState::StrongOpen,
                _ => ParseState::Complete,
            },
            ParseState::Escape => ParseState::Complete,
            ParseState::Complete => match c {
                c if c.is_whitespace() && c != '\n' && c != '\r' => ParseState::WhiteSpace,
                '\\' => ParseState::Escape,
                '"' => ParseState::WeakOpen,
                '\'' => ParseState::StrongOpen,
                _ => ParseState::Complete,
            },
            ParseState::WeakOpen => match c {
                '\\' => ParseState::WeakOpenEscape,
                '"' => ParseState::Complete,
                _ => ParseState::WeakOpen,
            },
            ParseState::WeakOpenEscape => ParseState::WeakOpen,
            ParseState::StrongOpen => match c {
                '\\' => ParseState::StrongOpenEscape,
                '\'' => ParseState::Complete,
                _ => ParseState::StrongOpen,
            },
            ParseState::StrongOpenEscape => ParseState::StrongOpen,
            // Comments last the entire line.
            ParseState::Comment => panic!("should have returned early"),
        };
    }
    (cur_pos, cur_state)
}

impl<B: BufRead> Iterator for QuotedLines<B> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Result<String>> {
        let mut buf = String::new();
        let mut cur_state = ParseState::Complete;
        let mut buf_pos;
        let mut cur_pos;
        loop {
            buf_pos = buf.len();
            match self.buf.read_line(&mut buf) {
                Ok(0) => match cur_state {
                    ParseState::Complete => return None,
                    _ => {
                        let len = buf.len();
                        return Some(Err(DotenvParseError::LineParse(buf, len)));
                    }
                },
                Ok(_n) => {
                    // Skip lines which start with a # before iteration
                    // This optimizes parsing a bit.
                    if buf.trim_start().starts_with('#') {
                        return Some(Ok(String::with_capacity(0)));
                    }
                    let result = eval_end_state(cur_state, &buf[buf_pos..]);
                    cur_pos = result.0;
                    cur_state = result.1;

                    match cur_state {
                        ParseState::Complete => {
                            if buf.ends_with('\n') {
                                buf.pop();
                                if buf.ends_with('\r') {
                                    buf.pop();
                                }
                            }
                            return Some(Ok(buf));
                        }
                        ParseState::Escape
                        | ParseState::StrongOpen
                        | ParseState::StrongOpenEscape
                        | ParseState::WeakOpen
                        | ParseState::WeakOpenEscape
                        | ParseState::WhiteSpace => {}
                        ParseState::Comment => {
                            buf.truncate(buf_pos + cur_pos);
                            return Some(Ok(buf));
                        }
                    }
                }
                Err(e) => return Some(Err(DotenvParseError::Io(e))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use relux_core::pure::Env;
    use relux_core::pure::LayeredEnv;
    use relux_core::pure::LayeredEnvBuilder;
    use relux_core::pure::LayeredEnvSource;

    use super::parse_env;

    fn builder_over(pairs: &[(&str, &str)]) -> LayeredEnvBuilder {
        let mut env = Env::new();
        for (k, v) in pairs {
            env.insert((*k).into(), (*v).into());
        }
        let parent = Arc::new(LayeredEnv::root(env));
        LayeredEnvBuilder::new(parent, LayeredEnvSource::DotEnv("t/.env".into()))
    }

    fn parse(input: &str, parent_pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut b = builder_over(parent_pairs);
        parse_env(input.as_bytes(), &mut b).expect("parse ok")
    }

    // --- dialect smoke tests (catch a broken paste) ---

    #[test]
    fn plain_key_value() {
        assert_eq!(
            parse("A=1\nB=2\n", &[]),
            vec![("A".into(), "1".into()), ("B".into(), "2".into()),]
        );
    }

    #[test]
    fn export_prefix() {
        assert_eq!(parse("export A=1\n", &[]), vec![("A".into(), "1".into())]);
    }

    #[test]
    fn comments_and_blanks_skipped() {
        assert_eq!(parse("# c\n\nA=1\n", &[]), vec![("A".into(), "1".into())]);
    }

    #[test]
    fn double_quotes() {
        assert_eq!(
            parse("A=\"hello world\"\n", &[]),
            vec![("A".into(), "hello world".into())]
        );
    }

    #[test]
    fn single_quotes_are_literal() {
        // strong quotes suppress substitution
        assert_eq!(
            parse("A='$B literal'\n", &[("B", "x")]),
            vec![("A".into(), "$B literal".into())]
        );
    }

    #[test]
    fn escaped_quote_and_newline() {
        assert_eq!(
            parse("A=\"a\\\"b\\nc\"\n", &[]),
            vec![("A".into(), "a\"b\nc".into())]
        );
    }

    #[test]
    fn multiline_quoted_value() {
        assert_eq!(
            parse("A=\"line1\nline2\"\n", &[]),
            vec![("A".into(), "line1\nline2".into())]
        );
    }

    // --- substitution: our edits (highest value) ---

    #[test]
    fn substitution_resolves_from_parent() {
        assert_eq!(
            parse("A=${B}/x\n", &[("B", "root")]),
            vec![("A".into(), "root/x".into())]
        );
    }

    #[test]
    fn substitution_undefined_is_empty() {
        assert_eq!(
            parse("A=${MISSING}x\n", &[]),
            vec![("A".into(), "x".into())]
        );
    }

    #[test]
    fn substitution_same_file_earlier_line_wins() {
        // B defined earlier in the file shadows the parent's B
        assert_eq!(
            parse("B=local\nA=${B}\n", &[("B", "parent")]),
            vec![("B".into(), "local".into()), ("A".into(), "local".into()),]
        );
    }

    #[test]
    fn substitution_never_reads_process_env() {
        // The single most important test: proves the std::env::var removal.
        let key = "RELUX_M2_ENVLEAK_PROBE";
        // SAFETY: single-threaded test setting a uniquely-named var.
        unsafe {
            std::env::set_var(key, "LEAKED");
        }
        let got = parse(&format!("A=${{{key}}}z\n"), &[]);
        // std::env has the var, but the builder does not -> expands to empty.
        assert_eq!(got, vec![("A".into(), "z".into())]);
        unsafe {
            std::env::remove_var(key);
        }
    }

    // --- API / errors ---

    #[test]
    fn malformed_line_is_error() {
        let mut b = builder_over(&[]);
        // a bare word with no '=' is a parse error
        assert!(parse_env("not_an_assignment\n".as_bytes(), &mut b).is_err());
    }

    #[test]
    fn bom_is_stripped() {
        let input = b"\xEF\xBB\xBFA=1\n";
        let mut b = builder_over(&[]);
        assert_eq!(
            parse_env(&input[..], &mut b).unwrap(),
            vec![("A".into(), "1".into())]
        );
    }
}
