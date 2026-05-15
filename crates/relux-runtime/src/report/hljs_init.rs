//! Relux language definition for highlight.js. Lifted from
//! `docs/reference/highlight-relux.js` (which serves the mdBook docs).
//! Spliced into `event.html` as a `<script>` tag between the hljs
//! core bundle and the viewer bundle.

pub const HLJS_RELUX_INIT: &str = r#"
hljs.registerLanguage("relux", function(hljs) {
  var INTERPOLATION = {
    className: "subst",
    begin: /\$\{/, end: /\}/,
    contains: []
  };

  var CAPTURE_VAR = {
    className: "subst",
    begin: /\$[0-9]+/
  };

  var STRING = {
    className: "string",
    begin: '"', end: '"',
    contains: [
      hljs.BACKSLASH_ESCAPE,
      INTERPOLATION,
      CAPTURE_VAR
    ]
  };

  var DOCSTRING = {
    className: "string",
    begin: '"""', end: '"""',
    contains: []
  };

  var COMMENT = hljs.COMMENT("//", "$");

  return {
    name: "Relux",
    aliases: ["relux"],
    case_insensitive: false,
    keywords: {
      keyword: "test effect fn pure import shell let start expect expose var as cleanup needs if unless",
      built_in: "match_prompt match_exit_code match_not_ok sleep log uuid ctrl_c ctrl_d ctrl_z env default which concat upper lower trim trim_start trim_end replace starts_with ends_with contains split_first split_last"
    },
    contains: [
      COMMENT,
      DOCSTRING,
      {
        className: "meta",
        begin: /^\s*#\s+(skip|run|flaky)\b/,
        end: /$/
      },
      {
        begin: /=>/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /=>/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /(?<![=<!>])>/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: />/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /<[~@][0-9][0-9a-zA-Z]*[?=]/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<[~@][0-9][0-9a-zA-Z]*[?=]/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /<\?/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<\?/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /<=/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<=/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /!\?/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /!\?/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      {
        begin: /!=/, end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /!=/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },
      { className: "number", begin: /[~@][0-9][0-9a-zA-Z]*\b/ },
      { className: "type", begin: /\b[A-Z][a-zA-Z0-9_]*\b/ },
      STRING,
      { className: "number", begin: /\b[0-9]+\b/ }
    ]
  };
});
"#;
