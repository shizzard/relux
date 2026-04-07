// highlight.js v10 language definition for Relux DSL
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
    begin: '"""', end: '"""'
  };

  var COMMENT = hljs.COMMENT("//", "$");

  return {
    name: "Relux",
    aliases: ["relux"],
    case_insensitive: false,
    keywords: {
      keyword: "test effect fn pure import shell let start expect expose as cleanup needs if unless",
      built_in: "match_prompt match_exit_code match_not_ok sleep log uuid ctrl_c ctrl_d ctrl_z env default which concat upper lower trim trim_start trim_end replace starts_with ends_with contains split_first split_last"
    },
    contains: [
      COMMENT,
      DOCSTRING,

      // condition markers: # skip, # run, # flaky
      {
        className: "meta",
        begin: /^\s*#\s+(skip|run|flaky)\b/,
        end: /$/
      },

      // send raw operator => payload
      {
        begin: /=>/,
        end: /$/,
        className: "string",
        contains: [INTERPOLATION, CAPTURE_VAR],
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /=>/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // send operator > payload
      {
        begin: /(?<![=<!>])>/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: />/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // timed match operators <~5s= <~5s? <@2s= <@2s?
      {
        begin: /<[~@][0-9][0-9a-zA-Z]*[?=]/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<[~@][0-9][0-9a-zA-Z]*[?=]/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // match regex <?
      {
        begin: /<\?/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<\?/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // match literal <=
      {
        begin: /<=/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /<=/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // fail regex !?
      {
        begin: /!\?/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /!\?/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // fail literal !=
      {
        begin: /!=/,
        end: /$/,
        returnBegin: true,
        contains: [
          { className: "keyword", begin: /!=/ },
          { className: "string", begin: /./, end: /$/, contains: [INTERPOLATION, CAPTURE_VAR] }
        ]
      },

      // timeout values ~5s @2s
      {
        className: "number",
        begin: /[~@][0-9][0-9a-zA-Z]*\b/
      },

      // effect identifiers (CamelCase)
      {
        className: "type",
        begin: /\b[A-Z][a-zA-Z0-9_]*\b/
      },

      STRING,

      // numbers
      {
        className: "number",
        begin: /\b[0-9]+\b/
      }
    ]
  };
});

// Re-highlight relux blocks (mdBook's book.js runs hljs before this script loads)
document.querySelectorAll('code.language-relux').forEach(function(block) {
  hljs.highlightBlock(block);
});
