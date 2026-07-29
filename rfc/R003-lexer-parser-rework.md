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

Before lexing, the input is normalized: `\r\n` → `\n`, stray `\r` removed. Spans refer to positions in the normalized source. The diagnostic renderer (Ariadne) receives the same normalized source, so spans are consistent. This is the standard approach used by Rust, Go, Python, JavaScript (V8), and Swift.

### Token set

Every token carries its source slice (`&str`) and byte span. The lexer emits a small, fixed set: single-character symbols (`$ { } ( ) " < > = ! ? ~ @ \ # [ ] , / -`), an `Escape` token (backslash plus any non-newline character, carrying the escaped character), whitespace (`Space`, `Tab`, `Newline`), and the keywords (`fn`, `pure`, `effect`, `test`, `shell`, `let`, `need`, `import`, `cleanup`, `as`). Anything the lexer does not recognize becomes a catch-all `Text` token, with adjacent unmatched characters squashed into one.

The `Text` catch-all is the key move: it is defined by what it is *not*, so adding a new symbol token automatically excludes it from `Text` with no exclusion regex to maintain, and any valid UTF-8 that is not a recognized token becomes `Text` (Unicode-safe by construction, lowest priority by construction).

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

The parser validates identifiers by converting `Text` tokens to typed wrappers using fallible `TryInto` conversions (`EffectIdent` for CamelCase, `FnIdent` for snake_case, `VarIdent` for variable rules). Since tokens carry both `&str` and span, the conversion produces a typed identifier with its span, or an error with the same span for diagnostics. The validation rules live in one place per identifier type, not scattered across lexer regexes.

## Parser design

The parser is built with [Chumsky 0.12](https://docs.rs/chumsky/0.12) combinators over the lexer's `&[Spanned<Token>]` output, composed bottom-up from leaf token matchers through interpolation, expressions, statements, annotations, and definitions up to the module. The context-dependent work the old lexer did by morphing modes — operator recognition, payload collection, string parsing, escape interpretation, marker parsing, identifier classification — all moves here, where grammar context is known. AST nodes capture their spans directly from token positions, which removes the whole class of span-offset bugs the old architecture produced.

The one AST-level design change: send / match / timed-match / buffer-reset operations become first-class `Stmt` variants (`Stmt::Send`, `Stmt::MatchRegex`, `Stmt::TimedMatchRegex`, `Stmt::BufferReset`, etc.) instead of being routed through `Stmt::Expr(AstExpr)`. This separates side-effecting statements from value expressions; the `TimeoutKind` (tolerance vs assertion) is carried in a field rather than duplicated across separate assert variants. The resolver is updated to consume the restructured AST.

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

This rework provides the parser infrastructure needed for R002 (Best-Effort Cleanup). The cleanup-block parsing and ordered body sections in the effect and test definitions are designed to accommodate cleanup semantics. Implementing R003 first unblocks R002 without further parser changes.
