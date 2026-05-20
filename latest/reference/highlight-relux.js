// Canonical highlight.js v11 grammar for the Relux DSL.
//
// Embedded into `event.html` by the Rust runtime via `include_str!`
// in `crates/relux-runtime/src/report/hljs_init.rs`. Copied into each
// mdBook directory by the `just books` target.
//
// Emitted hljs classes (CSS lives in viewer/src/components/sections/
// SourceView.svelte for the runtime, and in each mdBook's bundled
// theme for the docs):
//
//   hljs-keyword  - declaration words, shell ops, match ops, durations (with sub-class)
//   hljs-duration - opt-out-of-bold sub-class for ~5s / @2s tokens
//   hljs-variable - all variable refs (decl sites, ${...} interior, $1 captures, dotted members)
//   hljs-subst    - the ${...} interpolation chrome
//   hljs-type     - CamelCase effect names and aliases at every site
//   hljs-title    - function declaration names; rendered as `hljs-title hljs-function`
//   hljs-built_in - BIF call names
//   hljs-string   - quoted strings, docstrings, shell payloads
//   hljs-number   - bare integer literals
//   hljs-comment  - line comments AND marker outer chrome
//
// IMPORTANT: split-scope rules MUST use the array form of `match`/`begin`
// (e.g. `match: [re1, re2, re3]`). The single-regex-with-capture-groups
// form silently fails to attach scopes in hljs v11. The OP_SEND / OP_*
// rules use a single-string `beginScope` and are fine with a single
// regex; the timed-match split rules use object `beginScope` and must
// use the array form.
hljs.registerLanguage("relux", function (hljs) {
  const INTERPOLATION = {
    className: "subst",
    begin: /\$\{/,
    end: /\}/,
    contains: [
      { className: "variable", begin: /[a-zA-Z_][a-zA-Z0-9_]*/ },
      { begin: /\./ }
    ]
  };

  const CAPTURE_VAR = { className: "variable", begin: /\$[0-9]+/ };

  const STRING = {
    className: "string",
    begin: '"',
    end: '"',
    contains: [hljs.BACKSLASH_ESCAPE, INTERPOLATION, CAPTURE_VAR]
  };

  const DOCSTRING = { className: "string", begin: '"""', end: '"""' };

  const COMMENT = hljs.COMMENT("//", "$");

  const PAYLOAD_CONTAINS = [INTERPOLATION, CAPTURE_VAR];

  // Standalone timeout: ~5s or @2s on its own. Class `keyword.duration`
  // emits both `hljs-keyword` (for the amber color rule) and `hljs-duration`
  // (for the un-bold rule).
  const DURATION = { scope: "keyword.duration", begin: /[~@][0-9][0-9a-zA-Z]*\b/ };

  // Marker line: `# skip|run|flaky [if|unless ...]`. The outer mode is
  // a comment; inner contains rules light up keywords, match ops, function
  // calls, variables, and strings against the comment-faint background.
  const MARKER = {
    className: "comment",
    begin: /^\s*#\s+(?=(?:skip|run|flaky)\b)/,
    end: /$/,
    contains: [
      { className: "keyword", begin: /\b(?:skip|run|flaky|if|unless)\b/ },
      { className: "keyword", begin: /\s(?:\?=|=)(?=\s)/ },
      { className: "title", begin: /\b[a-z_][a-zA-Z0-9_]*(?=\()/ },
      INTERPOLATION,
      CAPTURE_VAR,
      STRING,
      { className: "variable", begin: /\b[a-zA-Z_][a-zA-Z0-9_]*\b/ }
    ]
  };

  // Shell-operator + payload pairs. Each emits the operator characters
  // as `keyword` and the rest of the line as `string` with interpolation
  // sub-rules. Order matters: longest / most-specific first.
  //
  // The timed-match variants use the array form of `begin` + object
  // `beginScope` to split the operator into three pieces (open bracket
  // / duration / close char) without giving the duration segment the
  // outer `keyword` boldness.

  const OP_TIMED_MATCH_REGEX = {
    begin: [/</, /[~@][0-9][0-9a-zA-Z]*/, /\?/],
    beginScope: { 1: "keyword", 2: "keyword.duration", 3: "keyword" },
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_TIMED_MATCH_LITERAL = {
    begin: [/</, /[~@][0-9][0-9a-zA-Z]*/, /=/],
    beginScope: { 1: "keyword", 2: "keyword.duration", 3: "keyword" },
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_MATCH_REGEX = {
    begin: /<\?/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_MATCH_LITERAL = {
    begin: /<=/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_FAIL_REGEX = {
    begin: /!\?/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_FAIL_LITERAL = {
    begin: /!=/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_SEND_RAW = {
    begin: /=>/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  const OP_SEND = {
    begin: /(?<![-=<!)>])>/,
    beginScope: "keyword",
    end: /$/,
    contains: PAYLOAD_CONTAINS,
    className: "string"
  };

  // Declaration-site split-scope rules. ALL use the array form of `match`
  // so the object `scope` actually attaches per-piece classes.

  const DECL_FN = {
    match: [/\bfn/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "title.function" }
  };

  const DECL_EFFECT = {
    match: [/\beffect/, /\s+/, /[A-Z][a-z][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "type" }
  };

  const DECL_LET_VAR = {
    match: [/\b(?:let|var)/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "variable" }
  };

  // `expose shell <name>` — `shell` is a keyword here, not the bound name.
  // The longer form `expose shell <Type>.<member> as <alias>` falls through:
  // `expose` and `shell` get the keyword map, the dotted access is caught
  // by USE_DOTTED_TYPE, `as` gets the keyword map.
  const DECL_EXPOSE = {
    match: [/\bexpose/, /\s+/, /shell/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "keyword", 5: "variable" }
  };

  const DECL_EXPECT = {
    match: [/\bexpect/, /\s+/, /[a-zA-Z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "variable" }
  };

  const DECL_SHELL = {
    match: [/\bshell/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "variable" }
  };

  const DECL_START_AS = {
    match: [
      /\bstart/,
      /\s+/,
      /[A-Z][a-z][a-zA-Z0-9_]*/,
      /\s+/,
      /as/,
      /\s+/,
      /[A-Z][a-z][a-zA-Z0-9_]*/
    ],
    scope: { 1: "keyword", 3: "type", 5: "keyword", 7: "type" }
  };

  const DECL_START = {
    match: [/\bstart/, /\s+/, /[A-Z][a-z][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "type" }
  };

  const DECL_IMPORT = {
    match: [/\bimport/, /\s+/, /[a-zA-Z_][a-zA-Z0-9_/]*/],
    scope: { 1: "keyword", 3: "variable" }
  };

  // The `{ ... }` block that follows `import <path>`. Inside this block,
  // identifiers are either effects (CamelCase / short-uppercase aliases
  // like `E1`) or functions (lowercase). The lookbehind on `begin`
  // restricts activation to the import context — every other `{`/`}`
  // (effect bodies, test bodies, fn bodies, start blocks) is left alone.
  // The relaxed CamelCase regex (no required `[a-z]` second char) is
  // needed because import aliases are often short: `Foo as F`, `Bar as B`.
  const IMPORT_BRACE = {
    begin: /(?<=import\s+[a-zA-Z_][a-zA-Z0-9_/]*\s*)\{/,
    end: /\}/,
    contains: [
      { className: "type", begin: /\b[A-Z][a-zA-Z0-9_]*\b/ },
      {
        match: [/\bas/, /\s+/, /[A-Z][a-zA-Z0-9_]*/],
        scope: { 1: "keyword", 3: "type" }
      },
      {
        match: [/\bas/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
        scope: { 1: "keyword", 3: "title" }
      },
      { className: "title", begin: /\b[a-z_][a-zA-Z0-9_]*\b/ }
    ]
  };

  // Generic `as <name>` — catches the trailing rename in:
  //   expose var Db.port as db_port
  //   expose var id as script_id
  //   import service/db { url as db_url, StartDb }
  //   need StartAuth as auth
  // DECL_START_AS handles `start X as Y` before this fires.
  const DECL_AS_TYPE = {
    match: [/\bas/, /\s+/, /[A-Z][a-z][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "type" }
  };

  const DECL_AS_VAR = {
    match: [/\bas/, /\s+/, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "keyword", 3: "variable" }
  };

  // Assignment / reassignment LHS — catches bare `hostname = "x"` and the
  // env bindings inside `start X { K = V }`. The negative lookahead on
  // `=(?!>)` keeps `=>` (raw send) out of this rule.
  const ASSIGN_LHS = {
    match: [/\b[a-zA-Z_][a-zA-Z0-9_]*/, /\s*/, /=(?!>)/],
    scope: { 1: "variable" }
  };

  // Parenthesised argument / parameter lists. Activated by any `(...)`
  // — covers fn declaration params `fn foo(p1, p2)` AND call-site args
  // `trim(input)` / `${match_exit_code(0)}`.
  const FN_CALL_ARGS = {
    begin: /\(/,
    end: /\)/,
    contains: [
      INTERPOLATION,
      CAPTURE_VAR,
      STRING,
      { className: "title", begin: /\b[a-z_][a-zA-Z0-9_]*(?=\()/ },
      { className: "type", begin: /\b[A-Z][a-z][a-zA-Z0-9_]*\b/ },
      { className: "variable", begin: /\b[A-Z_][A-Z0-9_]*\b/ },
      { className: "variable", begin: /\b[a-z_][a-zA-Z0-9_]*\b/ },
      { className: "number", begin: /\b[0-9]+\b/ }
    ]
  };

  // Use-site rules.

  // Dotted access with CamelCase prefix (effect alias method call):
  //   Srv.shell  ->  type . variable
  const USE_DOTTED_TYPE = {
    match: [/\b[A-Z][a-z][a-zA-Z0-9_]*/, /\./, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "type", 3: "variable" }
  };

  // Dotted access with lowercase prefix (variable.member):
  //   foo.bar  ->  variable . variable
  const USE_DOTTED_VAR = {
    match: [/\b[a-z_][a-zA-Z0-9_]*/, /\./, /[a-z_][a-zA-Z0-9_]*/],
    scope: { 1: "variable", 3: "variable" }
  };

  // Bare CamelCase use site (effect type or alias).
  const USE_TYPE = { scope: "type", begin: /\b[A-Z][a-z][a-zA-Z0-9_]*\b/ };

  // SCREAMING_SNAKE env-var style names — `STORAGE_REGION`, `PORT`,
  // `_PRIVATE`. Catches the trailing items in `expect A, B, C` and the
  // env-named arguments passed to function calls. Order-wise this lives
  // after USE_TYPE, which is fine because USE_TYPE requires `[A-Z][a-z]`
  // (CamelCase has a lowercase second char), so `Server` and `STORAGE`
  // don't overlap.
  const USE_ENV = { scope: "variable", begin: /\b[A-Z_][A-Z0-9_]*\b/ };

  // Function call: a lowercase identifier immediately followed by `(`.
  const USE_FN_CALL = { scope: "title.function", begin: /\b[a-z_][a-zA-Z0-9_]*(?=\()/ };

  // Bare variable use site — a lowercase identifier that isn't a keyword,
  // a function call (followed by `(`), a dotted prefix (followed by `.`),
  // an assignment LHS (followed by `=`), or a block opener (followed by
  // `{`). Catches return values like `filename` on its own line, and
  // identifier uses in expression positions that no earlier rule covers.
  //
  // Contains rules fire BEFORE the keyword map, so the negative lookahead
  // at the start of the regex must list every language keyword that we
  // want to keep amber.
  const USE_VAR = {
    scope: "variable",
    begin: /\b(?!(?:test|effect|fn|pure|import|shell|let|start|expect|expose|var|as|cleanup|needs|if|unless)\b)[a-z_][a-zA-Z0-9_]*\b(?!\s*[(.={])/
  };

  return {
    name: "Relux",
    aliases: ["relux"],
    case_insensitive: false,
    keywords: {
      keyword:
        "test effect fn pure import shell let start expect expose var as cleanup needs if unless",
      built_in:
        // Pure BIFs (crates/relux-core/src/pure/bifs.rs)
        "trim upper lower replace split len uuid rand available_port which default " +
        // Impure BIFs (crates/relux-runtime/src/vm/bifs.rs)
        "sleep annotate log match_prompt match_exit_code match_ok match_not_ok " +
        "ctrl_c ctrl_d ctrl_z ctrl_l ctrl_backslash"
    },
    contains: [
      COMMENT,
      DOCSTRING,
      MARKER,

      // Declarations (most-specific first).
      DECL_START_AS,
      DECL_START,
      DECL_FN,
      DECL_EFFECT,
      DECL_LET_VAR,
      DECL_EXPOSE,
      DECL_EXPECT,
      DECL_SHELL,
      DECL_IMPORT,
      IMPORT_BRACE,
      DECL_AS_TYPE,
      DECL_AS_VAR,

      // Assignment / reassignment LHS.
      ASSIGN_LHS,

      // Parenthesised arg / param lists (fn decls and call sites).
      FN_CALL_ARGS,

      // Shell operator + payload pairs (longest first).
      OP_TIMED_MATCH_REGEX,
      OP_TIMED_MATCH_LITERAL,
      OP_MATCH_REGEX,
      OP_MATCH_LITERAL,
      OP_FAIL_REGEX,
      OP_FAIL_LITERAL,
      OP_SEND_RAW,
      OP_SEND,

      // Standalone timeout (must come before bare-number rule).
      DURATION,

      // Dotted access (must come before bare CamelCase and call-site rules
      // so the dotted form is preferred).
      USE_DOTTED_TYPE,
      USE_DOTTED_VAR,

      // Use sites.
      USE_TYPE,
      USE_ENV,
      USE_FN_CALL,
      USE_VAR,

      STRING,

      // Top-level capture var (`$1`..`$9`) — must come before the bare
      // number rule so the `$` doesn't get stranded and the `1` doesn't
      // get matched as a number.
      CAPTURE_VAR,

      // Bare integer literal. Same `--accent-2` color as strings.
      { className: "number", begin: /\b[0-9]+\b/ }
    ]
  };
});

// Re-highlight `code.language-relux` blocks. mdBook's `book.js` calls
// `hljs.highlightBlock` on every `<code>` element BEFORE this script
// runs (additional-js loads after book.js), so Relux blocks get
// processed as plain text — hljs marks them with the `hljs` class and
// `data-highlighted` and refuses to retry. Strip both markers and
// re-run with the now-registered grammar.
//
// In the viewer runtime report this loop is a no-op: SourceView calls
// `hljs.highlight(src, { language: 'relux' })` on a raw string and
// never produces `<code class="language-relux">` DOM nodes.
(function rehighlightRelux() {
  // mdBook's `book.js` calls `hljs.highlightBlock` on every `<code>`
  // element BEFORE this script runs (additional-js loads after
  // book.js), so `language-relux` blocks get processed as plain text
  // — hljs marks them with the `hljs` class and `data-highlighted`
  // and refuses to retry. Strip both markers and re-highlight with
  // the now-registered grammar. `highlightBlock` is deprecated in
  // v11.11.1 but still functional.
  //
  // In the viewer runtime report this loop is a no-op: SourceView
  // calls `hljs.highlight(src, { language: 'relux' })` on a raw string
  // and never produces `<code class="language-relux">` DOM nodes.
  function run() {
    document.querySelectorAll("code.language-relux").forEach(function (block) {
      block.classList.remove("hljs");
      delete block.dataset.highlighted;
      hljs.highlightBlock(block);
    });
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", run);
  } else {
    run();
  }
})();
