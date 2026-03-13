package eu.spawnlink.relux;

import com.intellij.psi.tree.IElementType;

/**
 * Token types for the Relux lexer.
 */
public interface ReluxTokenTypes {
    // Keywords
    IElementType TEST = new ReluxElementType("TEST");
    IElementType EFFECT = new ReluxElementType("EFFECT");
    IElementType FN = new ReluxElementType("FN");
    IElementType IMPORT = new ReluxElementType("IMPORT");
    IElementType SHELL = new ReluxElementType("SHELL");
    IElementType LET = new ReluxElementType("LET");
    IElementType NEED = new ReluxElementType("NEED");
    IElementType AS = new ReluxElementType("AS");
    IElementType CLEANUP = new ReluxElementType("CLEANUP");

    // Condition marker keywords
    IElementType SKIP = new ReluxElementType("SKIP");
    IElementType RUN = new ReluxElementType("RUN");
    IElementType FLAKY = new ReluxElementType("FLAKY");
    IElementType IF = new ReluxElementType("IF");
    IElementType UNLESS = new ReluxElementType("UNLESS");

    // Operators
    IElementType OP_SEND = new ReluxElementType("OP_SEND");                       // >
    IElementType OP_SEND_RAW = new ReluxElementType("OP_SEND_RAW");               // =>
    IElementType OP_MATCH_REGEX = new ReluxElementType("OP_MATCH_REGEX");         // <?
    IElementType OP_MATCH_LITERAL = new ReluxElementType("OP_MATCH_LITERAL");     // <=
    IElementType OP_FAIL_REGEX = new ReluxElementType("OP_FAIL_REGEX");           // !?
    IElementType OP_FAIL_LITERAL = new ReluxElementType("OP_FAIL_LITERAL");       // !=
    IElementType OP_ARROW = new ReluxElementType("OP_ARROW");                     // ->
    IElementType OP_ASSIGN = new ReluxElementType("OP_ASSIGN");                   // =

    // Timeout
    IElementType TIMEOUT = new ReluxElementType("TIMEOUT");                       // ~5s

    // Punctuation
    IElementType LBRACE = new ReluxElementType("LBRACE");
    IElementType RBRACE = new ReluxElementType("RBRACE");
    IElementType LPAREN = new ReluxElementType("LPAREN");
    IElementType RPAREN = new ReluxElementType("RPAREN");
    IElementType LBRACKET = new ReluxElementType("LBRACKET");
    IElementType RBRACKET = new ReluxElementType("RBRACKET");
    IElementType COMMA = new ReluxElementType("COMMA");

    // Literals
    IElementType NUMBER = new ReluxElementType("NUMBER");
    IElementType STRING = new ReluxElementType("STRING");
    IElementType DOCSTRING = new ReluxElementType("DOCSTRING");
    IElementType INTERPOLATION = new ReluxElementType("INTERPOLATION");
    IElementType CAPTURE_VAR = new ReluxElementType("CAPTURE_VAR");               // $0, $1, etc.
    IElementType COMMENT = new ReluxElementType("COMMENT");

    // Identifiers
    IElementType IDENTIFIER = new ReluxElementType("IDENTIFIER");                 // lowercase
    IElementType EFFECT_IDENTIFIER = new ReluxElementType("EFFECT_IDENTIFIER");   // PascalCase
    IElementType MODULE_PATH = new ReluxElementType("MODULE_PATH");               // path/to/module

    // Payloads (content after operators)
    IElementType PAYLOAD = new ReluxElementType("PAYLOAD");
    IElementType REGEX_PAYLOAD = new ReluxElementType("REGEX_PAYLOAD");

    // Condition values
    IElementType CONDITION_OP = new ReluxElementType("CONDITION_OP");             // = or ?
    IElementType CONDITION_VALUE = new ReluxElementType("CONDITION_VALUE");
    IElementType ENV_VAR = new ReluxElementType("ENV_VAR");

    // Whitespace and bad character
    IElementType WHITESPACE = new ReluxElementType("WHITESPACE");
    IElementType BAD_CHARACTER = new ReluxElementType("BAD_CHARACTER");
}
