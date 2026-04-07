# R003: Lexer/Parser Rework

- **Status**: implemented
- **Created**: 2026-03-19

## Motivation

The current lexer uses Logos sub-morphing to switch between context-dependent modes (`UnquotedMode`, `QuotedMode`, `DocStringMode`) during tokenization. Each mode has hand-crafted character-class exclusion regexes that must stay synchronized with token definitions in the same enum. This creates a class of bugs where adding or forgetting a character in an exclusion set causes silent misparsing.

Known bugs caused by this architecture:

- **`$"` in quoted strings**: `QuotedMode`'s `LiteralDollar` regex `\$[^{\n$]` matches `$"` because `"` is not in the exclusion set. This consumes the closing quote, so the string is never closed.
- **Timed match span math**: prefix length is hardcoded to `2` instead of computed from the actual operator prefix length, causing error spans to point at wrong source locations.

The lexer also embeds a recursive descent parser for condition markers (`[skip if ...]`), performs escape sequence interpretation, and resolves interpolation fragments — all work that belongs in the parser where grammar context is known.

The fundamental problem is that the lexer is context-aware: it produces different token types depending on whether it's inside a string, an operator payload, or a marker expression. This makes it fragile, hard to extend, and the source of span-tracking bugs (Logos's `morph()` corrupts internal position state).

## Syntax changes

The rework includes two user-visible syntax changes that simplify the lexer/parser boundary:

### Comments: `#` → `//`

Comments change from `# comment` to `// comment`. This frees `#` for use as the marker prefix.

### Markers: `[...]` → `# ...`

Condition markers change from bracket-delimited to line-based:

```
// Before:
[skip if "${CI}" = "true"]
[flaky]
test "fragile test" { ... }

// After:
# skip if "${CI}" = "true"
# flaky
test "fragile test" { ... }
```

This eliminates the `]`-inside-regex ambiguity. The marker `[skip if "${VAR}" ? pattern[0-9]]` required the lexer to distinguish `]` as "end of regex character class" vs "end of marker" — impossible without context. With `#` prefix, the marker payload terminates at `Newline` like every other payload. No special delimiters, no nesting ambiguity.

## Lexer design

### Principle: context-free lexer, context-aware parser

The lexer becomes a simple tokenizer that recognizes individual characters and character runs without any semantic awareness. It does not know about operators, strings, interpolation, or markers. All context-dependent decisions move to the parser.

### Input normalization

Before lexing, the input is normalized: `\r\n` → `\n`, stray `\r` removed. Spans refer to positions in the normalized source. The diagnostic renderer (Ariadne) receives the same normalized source, so spans are consistent.

This is the standard approach used by Rust, Go, Python, JavaScript (V8), and Swift.

### Token set

Every token carries its source slice (`&str`) and byte span (`Range<usize>`).

#### Symbols (single character)

| Token | Character |
|-------|-----------|
| `Dollar` | `$` |
| `BraceOpen` | `{` |
| `BraceClose` | `}` |
| `ParenOpen` | `(` |
| `ParenClose` | `)` |
| `Quote` | `"` |
| `Lt` | `<` |
| `Gt` | `>` |
| `Eq` | `=` |
| `Bang` | `!` |
| `Question` | `?` |
| `Tilde` | `~` |
| `At` | `@` |
| `Backslash` | `\` (only at EOF, when no character follows) |
| `Hash` | `#` |
| `BracketOpen` | `[` |
| `BracketClose` | `]` |
| `Comma` | `,` |
| `Slash` | `/` |
| `Dash` | `-` |

#### Escape

| Token | Pattern | Payload |
|-------|---------|---------|
| `Escape` | `\.` (backslash + any non-newline char) | The character after the backslash (`&str`). The span covers both characters. |

`Escape` has higher priority than `Backslash`. The regex `.` excludes `\n`, so a backslash before a real newline produces `Backslash Newline` (two tokens). A backslash at EOF with no following character produces a bare `Backslash`.

`Display` for `Escape` renders the verbatim source representation (backslash + payload), so regex contexts can use it directly to pass escape sequences through uninterpreted.

#### Whitespace

| Token | Pattern |
|-------|---------|
| `Space` | ` +` |
| `Tab` | `\t+` |
| `Newline` | `\n` |

Whitespace tokens are emitted into the stream. The parser decides where they are significant (inside payloads and strings) and where they are ignorable (between structural tokens).

#### Keywords

`fn`, `pure`, `effect`, `test`, `shell`, `let`, `need`, `import`, `cleanup`, `as`

Keywords are recognized by exact match. Logos `#[token]` rules take priority over other patterns.

#### Text (catch-all)

`Text` is not a Logos token variant. It is produced by the post-lex processing loop: any input that Logos does not recognize as a symbol, whitespace, or keyword becomes `Text`. Adjacent unmatched characters are squashed into a single `Text` token (same logic as the current `Unrecognized` squashing).

This means:

- **No exclusion regex.** `Text` is defined by what it is *not* — anything that isn't a recognized token. Adding a new symbol token automatically excludes it from `Text` without modifying any regex.
- **Unicode-safe.** Any valid UTF-8 sequence that isn't a recognized token becomes `Text`. No charset assumptions.
- **Lowest priority by construction.** Logos tries all defined rules first; `Text` only captures the remainder.

### What the lexer does NOT do

- No sub-modes or morphing
- No interpolation recognition (`${var}` is `Dollar BraceOpen Text("var") BraceClose`)
- No escape interpretation (`\n` is `Escape("n")` — the lexer captures the pair but does not interpret it)
- No operator payload collection
- No string content parsing
- No marker expression parsing
- No identifier classification (CamelCase vs snake_case)
- No span correction after morph callbacks

### Identifier validation via TryInto

The parser validates identifiers by converting `Text` tokens to typed wrappers using fallible `TryInto` conversions. Since tokens carry both `&str` and `Span`, the conversion produces a typed identifier with its span, or an error with the same span for diagnostics.

```
token.try_into::<EffectIdent>()  // checks CamelCase
token.try_into::<FnIdent>()      // checks snake_case
token.try_into::<VarIdent>()     // checks variable naming rules
```

The validation rules live in one place per identifier type, not scattered across lexer regexes.

## Parser design

### Contract boundary

**Entry point** (unchanged):

```rust
pub fn parse(source: &str) -> Result<Module, ParseError>
```

**Unchanged AST types:**

- `Module`, `Item`, `TimeoutKind`, `Span`
- `EffectDef`, `EffectItem`
- `TestDef`, `TestItem`
- `AstInterpolation`, `CallExpr`
- `Import`, `ImportName`
- `NeedDecl`, `OverlayEntry`
- `MarkerDecl`, `MarkerKind`, `CondModifier`, `AstMarkerCond`, `AstMarkerCondBody`
- `LetStmt`, `AssignStmt`
- `ShellBlock`
- `ParseError`

**New AST variants** (to be added):

- `AstStringPart::CaptureRef(index)` — capture group reference in interpolation context (`${1}`)
- `AstExpr::CaptureRef(index)` — capture group reference in expression context (`$1`)

**Modified AST types:**

- `FnDef`, `PureFnDef` — add `markers: Vec<Spanned<MarkerDecl>>` field
- `CleanupBlock` — uses `Vec<Spanned<Stmt>>` instead of `Vec<Spanned<CleanupStmt>>`
- `Stmt` — gains first-class variants for shell operators (see below)
- `AstExpr` — operator variants removed (see below)

**Removed AST types:**

- `CleanupStmt` — replaced by `Stmt` (cleanup restrictions validated by resolver)

**AST restructuring — `Stmt`/`AstExpr` boundary:**

The current `Stmt` enum routes send/match/timed-match operations through `Stmt::Expr(AstExpr)`, with `AstExpr` carrying variants like `Send`, `SendRaw`, `MatchRegex`, `MatchLiteral`, `TimedMatchRegex`, `TimedMatchLiteral`, `BufferReset`. This conflates expressions with side-effecting statements.

The rework lifts these into first-class `Stmt` variants: `Stmt::Send`, `Stmt::SendRaw`, `Stmt::MatchRegex`, `Stmt::MatchLiteral`, `Stmt::TimedMatchRegex(kind, duration, payload)`, `Stmt::TimedMatchLiteral(kind, duration, payload)`, `Stmt::BufferReset`. The `TimeoutKind` (tolerance vs assertion) is carried in the `kind` field — there are no separate assert variants. The corresponding `AstExpr` variants are removed.

**Note:** The resolver must be updated to consume the restructured AST.

### Parser-internal types

Intermediate types used by combinators to pass structured values between layers. These are not part of the public AST — they exist only within the parser module.

```rust
/// Parsed timeout: kind (tolerance/assertion) + duration string.
struct ParsedTimeout {
    kind: TimeoutKind,
    duration: String,
}

/// Parsed name with optional alias: `Foo` or `Foo as Bar`.
struct AliasedName {
    name: Spanned<String>,
    alias: Option<Spanned<String>>,
}
```

### Combinators

The parser is built with [Chumsky 0.12](https://docs.rs/chumsky/0.12) combinators. The input type is `&[Spanned<Token>]` (the lexer's output). Each combinator listed below is a function returning `impl Parser<...>`.

**Span convention:** Combinators that produce AST nodes wrap them in `Spanned<T>` using `.map_with()` to capture the span from `MapExtra`. The Result column states the return type explicitly — `Spanned<T>` when the span is attached, bare `T` when it's not (e.g., intermediate values consumed by the next layer). Combinators that return `()` are used purely for their consuming side effect (whitespace, punctuation, prefixes).

Chumsky primitives used throughout:

- `just(token)` — match a single token by value (used to implement `token(kind)` and `keyword(k)`)
- `select!`/`select_ref!` — match and extract from token variants (used to implement `text()`, `any_token()`)
- `.then()` — sequence two parsers
- `.ignore_then()` / `.then_ignore()` — sequence, keeping only one side
- `choice(...)` — ordered alternation
- `.map()` / `.map_with()` — transform output (`.map_with()` provides span via `MapExtra`)
- `.try_map()` — fallible transform with error reporting
- `.or_not()` — optional match
- `.repeated()` — zero-or-more repetition
- `.separated_by()` — comma-separated lists with `.allow_trailing()`
- `.delimited_by()` — match between open/close delimiters
- `.filter()` — constrain matched tokens

#### Layer 0: Leaf combinators

These match tokens directly and do not delegate to any other combinator. Everything else is composed from these.

| Combinator | Chumsky basis | Matches | Returns |
|------------|---------------|---------|---------|
| `token(kind)` | `just(kind)` | Exactly one token of the given kind | `Spanned<&str>` |
| `text()` | `select_ref!` | A `Text` token | `Spanned<&str>` |
| `keyword(k)` | `just(k)` | A specific keyword (`fn`, `pure`, `effect`, `test`, `shell`, `let`, `need`, `import`, `cleanup`, `as`) | `Span` |
| `any_token()` | `select_ref!` | Any single token regardless of kind | `Spanned<&str>` |

#### Layer 1: Validated text combinators

Built from `text()` with `TryFrom` validation. Shared by both interpolation and expression layers. All return `Spanned<String>`.

| Combinator | Validation |
|------------|------------|
| `ident_var()` | `VarIdent` — variable naming rules |
| `ident_fn()` | `FnIdent` — snake_case function names |
| `ident_effect()` | `EffectIdent` — CamelCase effect names |
| `expr_numeric()` | `NumericLiteral` — unquoted numeric literal |

#### Layer 2: Primitive interpolation combinators

Built from leaf and validated text combinators. Only called by the interpolation context combinators in Layer 3. No other combinator calls them directly.

| Combinator | Tokens | Result |
|------------|--------|--------|
| `interp_escaped_dollar()` | `token(Dollar)` `token(Dollar)` | `AstStringPart::EscapedDollar` |
| `interp_var_ref()` | `token(Dollar)` `token(BraceOpen)` `ident_var()` `token(BraceClose)` | `AstStringPart::VarRef(name)` |
| `interp_capture_ref()` | `token(Dollar)` `token(BraceOpen)` `expr_numeric()` `token(BraceClose)` | `AstStringPart::CaptureRef(index)` |
| `interp_escape_seq()` | `token(Escape)` | Payload matched against escape table below. Interpreted escape or `InvalidEscape` error. |
| `interp_raw_escape_seq()` | `token(Escape)` | Verbatim source representation via `Display` (backslash + payload), emitted as `AstStringPart::Literal`. Used in regex contexts to pass escape sequences through to the regex engine uninterpreted. |

Valid payloads for `interp_escape_seq()`:

| Payload | Value |
|---------|-------|
| `n` | newline |
| `t` | tab |
| `r` | carriage return |
| `\` | backslash |
| `"` | double quote |
| `0` | null |
| `a` | bell (0x07) |
| `b` | backspace (0x08) |
| `f` | form feed (0x0C) |
| `v` | vertical tab (0x0B) |
| `e` | escape (0x1B) |

Any other `Escape` payload produces an `InvalidEscape` error.

#### Layer 2: Whitespace combinators

All return `()`.

| Combinator | Tokens |
|------------|--------|
| `ws()` | Zero or more `token(Space)` / `token(Tab)` |
| `leading_ws()` | Zero or more `token(Space)` / `token(Tab)` — same as `ws()`, used at start of line to consume indentation |
| `newline()` | `token(Newline)` |
| `docstring_delim()` | `token(Quote)` `token(Quote)` `token(Quote)` |

#### Layer 2: Structural punctuation combinators

Semantic aliases for structural delimiters and connectors.

| Combinator | Tokens |
|------------|--------|
| `punctuation_arrow()` | `token(Dash)` `token(Gt)` |
| `punctuation_brace_open()` | `token(BraceOpen)` |
| `punctuation_brace_close()` | `token(BraceClose)` |
| `punctuation_paren_open()` | `token(ParenOpen)` |
| `punctuation_paren_close()` | `token(ParenClose)` |
| `punctuation_comma()` | `token(Comma)` |

#### Layer 2: Aliased identifier combinators

Built from validated text combinators, `keyword(as)`, and `ws()`. Alias is optional in all three. All return `AliasedName`.

| Combinator | Tokens | Used in |
|------------|--------|---------|
| `ident_aliased_fn()` | `ident_fn()` [optional `ws()` `keyword(as)` `ws()` `ident_fn()`] | imports |
| `ident_aliased_effect()` | `ident_effect()` [optional `ws()` `keyword(as)` `ws()` `ident_effect()`] | imports |
| `ident_aliased_effect_shell()` | `ident_effect()` [optional `ws()` `keyword(as)` `ws()` `ident_var()`] | need declarations |

#### Layer 2: Annotation prefix combinators

All return `()`.

| Combinator | Tokens |
|------------|--------|
| `prefix_comment()` | `token(Slash)` `token(Slash)` |
| `prefix_marker()` | `token(Hash)` |

#### Layer 2: Timeout combinators

Duration text is not validated here — deferred to humantime at resolution time. All return `Spanned<ParsedTimeout>`.

| Combinator | Tokens |
|------------|--------|
| `timeout_tolerance()` | `token(Tilde)` `text()` |
| `timeout_assert()` | `token(At)` `text()` |
| `timeout()` | `timeout_tolerance()` \| `timeout_assert()` |

#### Layer 2: Shell operator combinators

Built from leaf combinators only. These recognize the DSL operators inside shell blocks. All return `Span` (covering the operator tokens).

| Combinator | Tokens |
|------------|--------|
| `op_send()` | `token(Gt)` |
| `op_send_raw()` | `token(Eq)` `token(Gt)` |
| `op_match_regex()` | `token(Lt)` `token(Question)` |
| `op_match_literal()` | `token(Lt)` `token(Eq)` |
| `op_fail_regex()` | `token(Bang)` `token(Question)` |
| `op_fail_literal()` | `token(Bang)` `token(Eq)` |

#### Layer 3: Timed shell operator combinators

Built from `timeout()` + leaf combinators. Return `Spanned<ParsedTimeout>` — the timeout from inner `timeout()`, with span extended to cover the full operator (`<` through `?`/`=`).

| Combinator | Tokens |
|------------|--------|
| `op_timed_match_literal()` | `token(Lt)` `timeout()` `token(Eq)` |
| `op_timed_match_regex()` | `token(Lt)` `timeout()` `token(Question)` |

#### Layer 3: Interpolation context combinators

Built from primitive interpolation combinators. These collect a sequence of string parts until a terminator token, coalescing adjacent literals.

The `terminator` parameter is a `Token` value passed to the function. The combinator collects content tokens until it encounters the terminator, then consumes the terminator and returns. The terminator is not included in the result.

| Combinator | Alternatives per iteration | Terminator |
|------------|---------------------------|------------|
| `interp_literal(terminator)` | `interp_escaped_dollar()`, `interp_var_ref()`, `interp_capture_ref()`, `interp_escape_seq()`, `any_token()` | `Newline` (payloads) or `Quote` (strings) |
| `interp_regex(terminator)` | `interp_escaped_dollar()`, `interp_var_ref()`, `interp_capture_ref()`, `interp_raw_escape_seq()`, `any_token()` | `Newline` |

Both return `Spanned<AstInterpolation>`. Adjacent `AstStringPart::Literal` parts are coalesced into a single part by combining spans and concatenating source slices.

#### Layer 3: Expression combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `plain_string()` | `token(Quote)` [any token except `Quote` and `Newline`]* — consumes closing `Quote` | `Spanned<String>` (literal text, no interpolation or escape interpretation) |
| `expr_string()` | `token(Quote)` `interp_literal(Quote)` | `AstExpr::Interpolation` |
| `expr_call()` | `ident_fn()` `punctuation_paren_open()` [comma-separated `expr()`] `punctuation_paren_close()` | `AstExpr::Call` |
| `expr_capture_ref()` | `token(Dollar)` `expr_numeric()` | `AstExpr::CaptureRef(index)` |
| `expr_ident()` | `ident_var()` | `AstExpr::VarRef(name)` |
| `expr()` | tries `expr_call()`, `expr_string()`, `expr_capture_ref()`, `expr_numeric()`, `expr_ident()` | `AstExpr` |

`plain_string()` is used only for test names — positions where the string is a static label, not a runtime value. `expr_numeric()` produces `AstExpr::Interpolation` with a single `AstStringPart::Literal` — all values are strings in this DSL.

#### Layer 4: Statement combinators

Built from operator, interpolation, and expression combinators.

| Combinator | Built from | Result |
|------------|------------|--------|
| `stmt_send()` | `op_send()` `ws()` `interp_literal(Newline)` | `Stmt::Send` |
| `stmt_send_raw()` | `op_send_raw()` `ws()` `interp_literal(Newline)` | `Stmt::SendRaw` |
| `stmt_match_regex()` | `op_match_regex()` `ws()` `interp_regex(Newline)` | `Stmt::MatchRegex` |
| `stmt_match_literal()` | `op_match_literal()` `ws()` `interp_literal(Newline)` | `Stmt::MatchLiteral` |
| `stmt_fail_regex()` | `op_fail_regex()` `ws()` `interp_regex(Newline)` | `Stmt::FailRegex` |
| `stmt_fail_literal()` | `op_fail_literal()` `ws()` `interp_literal(Newline)` | `Stmt::FailLiteral` |
| `stmt_timed_match_literal()` | `op_timed_match_literal()` `ws()` `interp_literal(Newline)` | `Stmt::TimedMatchLiteral(kind, duration, payload)` |
| `stmt_timed_match_regex()` | `op_timed_match_regex()` `ws()` `interp_regex(Newline)` | `Stmt::TimedMatchRegex(kind, duration, payload)` |
| `stmt_timeout()` | `timeout()` `newline()` | `Stmt::Timeout(kind, duration)` |
| `stmt_let()` | `keyword(let)` `ws()` `ident_var()` [optional `ws()` `token(Eq)` `ws()` `expr()`] `newline()` | `Stmt::Let` |
| `stmt_assign()` | `ident_var()` `ws()` `token(Eq)` `ws()` `expr()` `newline()` | `Stmt::Assign` |
| `stmt_expr()` | `expr()` `newline()` | `Stmt::Expr` |
| `stmt()` | `leading_ws()` then tries alternatives in this order (longer prefixes first): `comment()`, `stmt_timed_match_literal()`, `stmt_timed_match_regex()`, `stmt_match_regex()`, `stmt_match_literal()`, `stmt_send_raw()`, `stmt_send()`, `stmt_fail_regex()`, `stmt_fail_literal()`, `stmt_timeout()`, `stmt_let()`, `stmt_assign()`, `stmt_expr()` | `Stmt` |

**Try order rationale:** `<~5s=` must be tried before `<=` (both start with `<`). `=>` must be tried before `>` (both contain `>`). `comment()` is first because `//` is unambiguous and cheap to reject. `stmt_let()` before `stmt_assign()` because `let x = ...` starts with a keyword. `stmt_expr()` is last — it's the catch-all for bare function calls.

Empty operator payloads: `op_match_regex()` or `op_match_literal()` followed by only whitespace and `newline()` → `Stmt::BufferReset`. `op_fail_regex()` or `op_fail_literal()` followed by only whitespace and `newline()` → `Stmt::ClearFailPattern`.

#### Layer 4: Annotation combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `comment()` | `prefix_comment()` tokens until `newline()` — consumes the newline | `String` — comment text (newline not included in result) |
| `docstring()` | `docstring_delim()` tokens until `docstring_delim()` — consumes the closing `"""` | `Spanned<String>` — plain text, no interpolation. Single and double `Quote` tokens inside the body are valid content; only three consecutive `Quote` tokens close the docstring. |
| `marker_cond()` | `expr()` then one of: (a) nothing → `AstMarkerCondBody::Bare(expr)` (truthiness check), (b) `ws()` `token(Eq)` `ws()` `expr()` → `AstMarkerCondBody::Eq(lhs, rhs)`, (c) `ws()` `token(Question)` `ws()` `interp_regex(Newline)` → `AstMarkerCondBody::Regex(expr, pattern)` | `AstMarkerCondBody` |
| `marker()` | `prefix_marker()` `ws()` `text("skip"\|"run"\|"flaky")` [optional `ws()` `text("if"\|"unless")` `ws()` `marker_cond()`] `newline()` | `MarkerDecl` |

#### Layer 4: Overlay combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `overlay_entry()` | `ident_var()` `ws()` `token(Eq)` `ws()` `expr()` | `OverlayEntry` |
| `overlay()` | `punctuation_brace_open()` comma-separated `overlay_entry()` with optional trailing comma `punctuation_brace_close()` | `Vec<OverlayEntry>` |

#### Layer 5: Import combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `import_path()` | `text()` [`token(Slash)` `text()`]* | path string (e.g. `"lib/utils/greeter"`) |
| `import()` | `keyword(import)` `ws()` `import_path()` [optional `ws()` `punctuation_brace_open()` comma-separated [`ident_aliased_fn()` \| `ident_aliased_effect()`] with optional trailing comma `punctuation_brace_close()`] `newline()` | `Import` |

- `import lib/greeter` — imports everything from the module
- `import lib/greeter { greet, hello }` — selective functions
- `import lib/greeter { Db as Database, greet, }` — mixed effects and functions, trailing comma allowed

#### Layer 5: Need combinator

| Combinator | Built from | Result |
|------------|------------|--------|
| `need()` | `keyword(need)` `ws()` `ident_aliased_effect_shell()` [optional `ws()` `overlay()`] `newline()` | `NeedDecl` |

The alias from `ident_aliased_effect_shell()` provides the shell name (optional). The overlay provides key-value overrides (optional). Both are independently optional:

- `need Db` — no alias, no overlay
- `need Db as db` — alias only
- `need Db { PORT = "5433" }` — overlay only, no alias
- `need Db as db { PORT = "5433" }` — alias and overlay
- `need Db as db { PORT = "5433", HOST = "localhost", }` — trailing comma allowed

#### Layer 5: Shell block combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `shell_block()` | `keyword(shell)` `ws()` `ident_var()` `ws()` `punctuation_brace_open()` [`stmt()` \| `newline()`]* `leading_ws()` `punctuation_brace_close()` | `ShellBlock` |
| `cleanup_block()` | `keyword(cleanup)` `ws()` `punctuation_brace_open()` [`stmt()` \| `newline()`]* `leading_ws()` `punctuation_brace_close()` | `CleanupBlock` |

Cleanup blocks have no shell name — they run in a fresh implicit shell. Both use `stmt()` for their body. The parser does not restrict which statements appear in cleanup blocks — cleanup restrictions (only send/let) are validated by the resolver. This is intentional: R002 (Best-Effort Cleanup) will relax cleanup restrictions, and using `stmt()` now avoids a separate `CleanupStmt` type that would need to be widened later. Blank lines (extra newlines) are skipped between statements.

#### Layer 5: Function definition combinators

| Combinator | Built from | Result |
|------------|------------|--------|
| `def_fn()` | [`leading_ws()` (`marker()` \| `comment()`) \| `newline()`]* `leading_ws()` `keyword(fn)` `ws()` `ident_fn()` `punctuation_paren_open()` [comma-separated `ident_var()`] `punctuation_paren_close()` `ws()` `punctuation_brace_open()` [`stmt()` \| `newline()`]* `leading_ws()` `punctuation_brace_close()` | `FnDef` |
| `def_pure_fn()` | [`leading_ws()` (`marker()` \| `comment()`) \| `newline()`]* `leading_ws()` `keyword(pure)` `ws()` `keyword(fn)` `ws()` `ident_fn()` `punctuation_paren_open()` [comma-separated `ident_var()`] `punctuation_paren_close()` `ws()` `punctuation_brace_open()` [`stmt()` \| `newline()`]* `leading_ws()` `punctuation_brace_close()` | `PureFnDef` |

Both use `stmt()` for their body — pure function restrictions are validated by the resolver, not the parser. Zero parameters is valid: `fn foo() { ... }`.

**Note:** Function markers are new syntax. The parser accepts them, but the resolver must ignore them until marker semantics for functions are defined. `FnDef` and `PureFnDef` need a `markers: Vec<Spanned<MarkerDecl>>` field added (matching `EffectDef` and `TestDef`).

#### Layer 6: Effect definition combinator

| Combinator | Built from | Result |
|------------|------------|--------|
| `def_effect()` | [`leading_ws()` (`marker()` \| `comment()`) \| `newline()`]* `leading_ws()` `keyword(effect)` `ws()` `ident_effect()` `ws()` `punctuation_arrow()` `ws()` `ident_var()` `ws()` `punctuation_brace_open()` [`leading_ws()` (`need()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`stmt_let()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`shell_block()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`cleanup_block()` \| `comment()`) \| `newline()`]? `leading_ws()` `punctuation_brace_close()` | `EffectDef` |

The `-> shell_name` is always required — every effect exports a shell. The body sections are order-enforced by the parser: needs, then let declarations, then shells, then cleanup. Comments and blank lines are allowed between items in any section. `leading_ws()` consumes indentation before each item.

#### Layer 6: Test definition combinator

| Combinator | Built from | Result |
|------------|------------|--------|
| `def_test()` | [`leading_ws()` (`marker()` \| `comment()`) \| `newline()`]* `leading_ws()` `keyword(test)` `ws()` `plain_string()` [`ws()` `timeout()`]? `ws()` `punctuation_brace_open()` [`leading_ws()` (`docstring()` \| `comment()`) \| `newline()`]? [`leading_ws()` (`need()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`stmt_let()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`shell_block()` \| `comment()`) \| `newline()`]* [`leading_ws()` (`cleanup_block()` \| `comment()`) \| `newline()`]? `leading_ws()` `punctuation_brace_close()` | `TestDef` |

The test name is a plain quoted string (no interpolation — `plain_string()` collects all tokens between `Quote` delimiters as literal text). An optional timeout may appear between the test name and the opening brace: `test "name" ~5s { ... }` or `test "name" @10s { ... }`. The `timeout()` combinator is `timeout_tolerance() | timeout_assert()`.

The body sections are order-enforced by the parser: docstring, needs, variable declarations, shells, cleanup. Comments and blank lines are allowed between items in any section. `leading_ws()` consumes indentation before each item.

#### Layer 7: Module combinator

| Combinator | Built from | Result |
|------------|------------|--------|
| `module_item()` | `leading_ws()` then tries `import()`, `def_fn()`, `def_pure_fn()`, `def_effect()`, `def_test()`, `comment()` | `Item` |
| `module()` | [`module_item()` \| `newline()`]* | `Module` |

Module-level items can appear in any order. Blank lines (extra newlines) are skipped between items. The try order for `module_item()` alternatives is unspecified — each starts with a distinct keyword (`import`, `fn`, `pure`, `effect`, `test`, `//`) so there is no ambiguity regardless of order.

### Parser notes

The following notes supplement the combinator definitions above with rules that aren't captured by the combinator structure alone.

#### Terminator consumption

All terminatable combinators are self-contained: they consume their terminator token and exclude it from the result. This applies to `interp_literal()`, `interp_regex()`, `comment()`, `docstring()`, and `plain_string()`. The caller never needs to match the closing delimiter — calling the combinator consumes everything from content through terminator.

#### Whitespace handling

`Space` and `Tab` tokens are emitted by the lexer everywhere. The parser handles them differently depending on the combinator:

- **Structural combinators** (`module()`, `def_*()`, `shell_block()`, etc.): `ws()` skips `Space`/`Tab` between structural elements
- **Interpolation combinators** (`interp_literal()`, `interp_regex()`): `Space`/`Tab` tokens are preserved as literal content via the `any_token()` catch-all
- **Between operator prefix and payload**: leading whitespace after the operator is trimmed by the `ws()` call in `stmt_*` combinators (e.g., `> echo hello` — the space after `>` is not part of the payload)

#### Timed match span contiguity

No whitespace is allowed between `Tilde`/`At` and the duration `Text`, or between the duration and the `Question`/`Eq` terminator. The parser validates this by checking span contiguity in the `timeout_tolerance()` and `timeout_assert()` combinators.

#### Empty timed match payloads

A timed match operator with an empty payload (`<~5s?` or `<~5s=` followed by only whitespace and newline) is a parse error. Unlike untimed `<?`/`<=` (which produce `Stmt::BufferReset`), a timed buffer reset has no use — if there's a timeout, you're waiting for something.

#### `plain_string()` and escape tokens

`plain_string()` treats all tokens between quotes as literal text, including `Escape` tokens. An `Escape("n")` inside a test name produces the literal characters `\n` (via `Display`), not a newline. This is intentional — test names are static labels, not runtime-interpreted strings.

#### Implementation order

1. **AST changes** — modify `Stmt`, `AstExpr`, `FnDef`, `PureFnDef`, `CleanupBlock`; add `CaptureRef` variants; remove `CleanupStmt`. This breaks the resolver — stub it with `todo!()` until step 4.
2. **Parser-internal types** — add `ParsedTimeout`, `AliasedName`.
3. **Combinators, bottom-up** — implement Layer 0 through Layer 7 in order. Each layer only depends on layers below it. Write unit tests per layer as you go.
4. **Resolver update** — adapt the resolver to consume the restructured AST.

#### Removal of fragment types

The current lexer's fragment pipeline (`UnresolvedInterpolationFragment` → `RawLiteralInterpolationFragment` / `RegexInterpolationFragment` → `LiteralInterpolationFragment`) is removed entirely. The parser builds `AstInterpolation { parts: Vec<AstStringPart> }` directly from the token stream using the `interp_literal()` and `interp_regex()` combinators.

The `TryFrom` conversion from `RawLiteralInterpolationFragment` to `LiteralInterpolationFragment` (for escape validation) is replaced by inline validation in `interp_escape_seq()` — it interprets the escape immediately and emits an `InvalidEscape` error if unrecognized.

## Impact

### What changes

- **Lexer**: complete rewrite. ~1700 lines across 6 files → single file with minimal token definitions and the squashing loop.
- **Parser**: significant rework. Gains responsibility for operator recognition, payload collection, string parsing, escape handling, marker parsing, and identifier validation.
- **AST types**: `Stmt` gains first-class variants for send/match/timed-match/assert operations (previously routed through `AstExpr`). `CleanupBlock` uses `Stmt` instead of `CleanupStmt`. `FnDef`/`PureFnDef` gain `markers` field. `AstStringPart` and `AstExpr` gain `CaptureRef` variants.
- **Fragment types**: `UnresolvedInterpolationFragment`, `RawLiteralInterpolationFragment`, `RegexInterpolationFragment`, `LiteralInterpolationFragment` and their resolution pipeline are removed. The parser produces `AstInterpolation` directly.
- **DSL syntax**: comments change from `#` to `//`, markers change from `[...]` to `# ...`.
- **Resolver**: must be updated to consume the restructured AST (new `Stmt` variants, removed `AstExpr` operator variants, `Stmt`-based cleanup blocks).

### What stays the same

- Public parser entry point (`parse()` signature)
- IR types (resolver output)
- Runtime (consumes IR, unaffected)
- CLI (consumes runtime output, unaffected)

### Bugs resolved

- `$"` in quoted strings (no `QuotedMode` exclusion regex to get wrong)
- Timed match span math (no prefix length calculation — parser uses token spans directly)
- Marker sub-expression spans (parser tracks spans naturally, no offset propagation needed)
- `StringPart` spans all `0..0` (parser builds spans from token positions)
- Marker `]`-inside-regex ambiguity (markers are line-terminated, no bracket delimiters)

### Relationship to R002

This rework provides the parser infrastructure needed for R002 (Best-Effort Cleanup). The `cleanup_block()` combinator and ordered body sections in `def_effect()` and `def_test()` are designed to accommodate cleanup semantics. Implementing R003 first unblocks R002 without further parser changes.
