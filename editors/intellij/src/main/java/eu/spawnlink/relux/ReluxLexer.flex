package eu.spawnlink.relux;

import com.intellij.lexer.FlexLexer;
import com.intellij.psi.tree.IElementType;
import com.intellij.psi.TokenType;

%%

%class ReluxLexer
%implements FlexLexer
%unicode
%function advance
%type IElementType
%eof{  return;
%eof}

%{
    private int payloadStart = -1;
%}

// Character classes
CRLF = \r|\n|\r\n
WHITE_SPACE = [ \t]
SPACE = {WHITE_SPACE}+
LINE_SPACE = {WHITE_SPACE}*

DIGIT = [0-9]
LETTER = [a-zA-Z_]
LOWERCASE = [a-z_]
UPPERCASE = [A-Z]

// Identifiers
IDENTIFIER = {LOWERCASE}({LETTER}|{DIGIT})*
EFFECT_IDENTIFIER = {UPPERCASE}({LETTER}|{DIGIT})*
MODULE_PATH = {LETTER}({LETTER}|{DIGIT})*(\/{LETTER}({LETTER}|{DIGIT})*)+

// Duration/Timeout
DURATION = {DIGIT}+{LETTER}+({DIGIT}+{LETTER}+)*

// String interpolation
INTERPOLATION = \$\{{LETTER}({LETTER}|{DIGIT})*\}
CAPTURE_VAR = \${DIGIT}+

// Condition operators
CONDITION_OP = [=?]

%state IN_CONDITION_MARKER
%state IN_OPERATOR_PAYLOAD
%state IN_REGEX_PAYLOAD
%state IN_STRING
%state IN_DOCSTRING

%%

// Comments
<YYINITIAL> {
    "#" [^\r\n]*                { return ReluxTokenTypes.COMMENT; }
}

// Docstrings
<YYINITIAL> {
    \"\"\"                      { yybegin(IN_DOCSTRING); return ReluxTokenTypes.DOCSTRING; }
}

<IN_DOCSTRING> {
    \"\"\"                      { yybegin(YYINITIAL); return ReluxTokenTypes.DOCSTRING; }
    [^\"]+                      { return ReluxTokenTypes.DOCSTRING; }
    \"                          { return ReluxTokenTypes.DOCSTRING; }
}

// Strings
<YYINITIAL> {
    \"                          { yybegin(IN_STRING); return ReluxTokenTypes.STRING; }
}

<IN_STRING> {
    \"                          { yybegin(YYINITIAL); return ReluxTokenTypes.STRING; }
    {INTERPOLATION}             { return ReluxTokenTypes.INTERPOLATION; }
    \\. |
    [^\"\\\$]+                  { return ReluxTokenTypes.STRING; }
    \$                          { return ReluxTokenTypes.STRING; }
}

// Condition markers: [skip|run|flaky if|unless ...]
<YYINITIAL> {
    "["                         { yybegin(IN_CONDITION_MARKER); return ReluxTokenTypes.LBRACKET; }
}

<IN_CONDITION_MARKER> {
    "skip"                      { return ReluxTokenTypes.SKIP; }
    "run"                       { return ReluxTokenTypes.RUN; }
    "flaky"                     { return ReluxTokenTypes.FLAKY; }
    "if"                        { return ReluxTokenTypes.IF; }
    "unless"                    { return ReluxTokenTypes.UNLESS; }
    {CONDITION_OP}              { return ReluxTokenTypes.CONDITION_OP; }
    {IDENTIFIER}                { return ReluxTokenTypes.ENV_VAR; }
    [^\]\s=?]+                  { return ReluxTokenTypes.CONDITION_VALUE; }
    {SPACE}                     { return TokenType.WHITE_SPACE; }
    "]"                         { yybegin(YYINITIAL); return ReluxTokenTypes.RBRACKET; }
}

// Keywords (must come before identifiers)
<YYINITIAL> {
    "test"                      { return ReluxTokenTypes.TEST; }
    "effect"                    { return ReluxTokenTypes.EFFECT; }
    "fn"                        { return ReluxTokenTypes.FN; }
    "import"                    { return ReluxTokenTypes.IMPORT; }
    "shell"                     { return ReluxTokenTypes.SHELL; }
    "let"                       { return ReluxTokenTypes.LET; }
    "need"                      { return ReluxTokenTypes.NEED; }
    "as"                        { return ReluxTokenTypes.AS; }
    "cleanup"                   { return ReluxTokenTypes.CLEANUP; }
}

// Operators with inline timeout: <~5s?  <~10s=  <~5s!?  <~10s!=
<YYINITIAL> {
    "<" {LINE_SPACE} "~" {DURATION} {LINE_SPACE} "!?" { yybegin(IN_REGEX_PAYLOAD); return ReluxTokenTypes.OP_NEG_MATCH_REGEX; }
    "<" {LINE_SPACE} "~" {DURATION} {LINE_SPACE} "!=" { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_NEG_MATCH_LITERAL; }
    "<" {LINE_SPACE} "~" {DURATION} {LINE_SPACE} "?"  { yybegin(IN_REGEX_PAYLOAD); return ReluxTokenTypes.OP_MATCH_REGEX; }
    "<" {LINE_SPACE} "~" {DURATION} {LINE_SPACE} "="  { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_MATCH_LITERAL; }
}

// Shell operators (order matters for precedence)
<YYINITIAL> {
    "<!?"                       { yybegin(IN_REGEX_PAYLOAD); return ReluxTokenTypes.OP_NEG_MATCH_REGEX; }
    "<!="                       { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_NEG_MATCH_LITERAL; }
    "<?"                        { yybegin(IN_REGEX_PAYLOAD); return ReluxTokenTypes.OP_MATCH_REGEX; }
    "<="                        { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_MATCH_LITERAL; }
    "!?"                        { yybegin(IN_REGEX_PAYLOAD); return ReluxTokenTypes.OP_FAIL_REGEX; }
    "!="                        { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_FAIL_LITERAL; }
    "=>"                        { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_SEND_RAW; }
    ">"                         { yybegin(IN_OPERATOR_PAYLOAD); return ReluxTokenTypes.OP_SEND; }
}

// Operator payload (everything after operator until end of line)
<IN_OPERATOR_PAYLOAD> {
    {INTERPOLATION}             { return ReluxTokenTypes.INTERPOLATION; }
    {CAPTURE_VAR}               { return ReluxTokenTypes.CAPTURE_VAR; }
    \$\$                        { return ReluxTokenTypes.PAYLOAD; }
    [^\r\n\$]+                  { return ReluxTokenTypes.PAYLOAD; }
    \$                          { return ReluxTokenTypes.PAYLOAD; }
    {CRLF}                      { yybegin(YYINITIAL); return TokenType.WHITE_SPACE; }
}

// Regex payload (for match/fail operators)
<IN_REGEX_PAYLOAD> {
    {INTERPOLATION}             { return ReluxTokenTypes.INTERPOLATION; }
    {CAPTURE_VAR}               { return ReluxTokenTypes.CAPTURE_VAR; }
    [^\r\n\$]+                  { return ReluxTokenTypes.REGEX_PAYLOAD; }
    \$                          { return ReluxTokenTypes.REGEX_PAYLOAD; }
    {CRLF}                      { yybegin(YYINITIAL); return TokenType.WHITE_SPACE; }
}

// Timeout
<YYINITIAL> {
    "~" {DURATION}              { return ReluxTokenTypes.TIMEOUT; }
}

// Other operators
<YYINITIAL> {
    "->"                        { return ReluxTokenTypes.OP_ARROW; }
    "="                         { return ReluxTokenTypes.OP_ASSIGN; }
}

// Punctuation
<YYINITIAL> {
    "{"                         { return ReluxTokenTypes.LBRACE; }
    "}"                         { return ReluxTokenTypes.RBRACE; }
    "("                         { return ReluxTokenTypes.LPAREN; }
    ")"                         { return ReluxTokenTypes.RPAREN; }
    ","                         { return ReluxTokenTypes.COMMA; }
}

// Identifiers and module paths
<YYINITIAL> {
    {MODULE_PATH}               { return ReluxTokenTypes.MODULE_PATH; }
    {EFFECT_IDENTIFIER}         { return ReluxTokenTypes.EFFECT_IDENTIFIER; }
    {INTERPOLATION}             { return ReluxTokenTypes.INTERPOLATION; }
    {CAPTURE_VAR}               { return ReluxTokenTypes.CAPTURE_VAR; }
    {IDENTIFIER}                { return ReluxTokenTypes.IDENTIFIER; }
}

// Whitespace
{SPACE}                         { return TokenType.WHITE_SPACE; }
{CRLF}                          { return TokenType.WHITE_SPACE; }

// Bad characters
[^]                             { return TokenType.BAD_CHARACTER; }
