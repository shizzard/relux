package eu.spawnlink.relux;

import com.intellij.lexer.Lexer;
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors;
import com.intellij.openapi.editor.colors.TextAttributesKey;
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase;
import com.intellij.psi.TokenType;
import com.intellij.psi.tree.IElementType;
import org.jetbrains.annotations.NotNull;

import static com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey;

/**
 * Syntax highlighter for Relux language.
 */
public class ReluxSyntaxHighlighter extends SyntaxHighlighterBase {

    // Define text attribute keys
    public static final TextAttributesKey KEYWORD =
            createTextAttributesKey("RELUX_KEYWORD", DefaultLanguageHighlighterColors.KEYWORD);

    public static final TextAttributesKey COMMENT =
            createTextAttributesKey("RELUX_COMMENT", DefaultLanguageHighlighterColors.LINE_COMMENT);

    public static final TextAttributesKey STRING =
            createTextAttributesKey("RELUX_STRING", DefaultLanguageHighlighterColors.STRING);

    public static final TextAttributesKey DOCSTRING =
            createTextAttributesKey("RELUX_DOCSTRING", DefaultLanguageHighlighterColors.DOC_COMMENT);

    public static final TextAttributesKey OPERATOR =
            createTextAttributesKey("RELUX_OPERATOR", DefaultLanguageHighlighterColors.OPERATION_SIGN);

    public static final TextAttributesKey NUMBER =
            createTextAttributesKey("RELUX_NUMBER", DefaultLanguageHighlighterColors.NUMBER);

    public static final TextAttributesKey EFFECT_NAME =
            createTextAttributesKey("RELUX_EFFECT_NAME", DefaultLanguageHighlighterColors.CLASS_NAME);

    public static final TextAttributesKey IDENTIFIER =
            createTextAttributesKey("RELUX_IDENTIFIER", DefaultLanguageHighlighterColors.IDENTIFIER);

    public static final TextAttributesKey INTERPOLATION =
            createTextAttributesKey("RELUX_INTERPOLATION", DefaultLanguageHighlighterColors.INSTANCE_FIELD);

    public static final TextAttributesKey REGEX =
            createTextAttributesKey("RELUX_REGEX", DefaultLanguageHighlighterColors.VALID_STRING_ESCAPE);

    public static final TextAttributesKey MODULE_PATH =
            createTextAttributesKey("RELUX_MODULE_PATH", DefaultLanguageHighlighterColors.CLASS_REFERENCE);

    public static final TextAttributesKey BRACKETS =
            createTextAttributesKey("RELUX_BRACKETS", DefaultLanguageHighlighterColors.BRACKETS);

    public static final TextAttributesKey BRACES =
            createTextAttributesKey("RELUX_BRACES", DefaultLanguageHighlighterColors.BRACES);

    public static final TextAttributesKey PARENTHESES =
            createTextAttributesKey("RELUX_PARENTHESES", DefaultLanguageHighlighterColors.PARENTHESES);

    public static final TextAttributesKey COMMA =
            createTextAttributesKey("RELUX_COMMA", DefaultLanguageHighlighterColors.COMMA);

    public static final TextAttributesKey BAD_CHARACTER =
            createTextAttributesKey("RELUX_BAD_CHARACTER", DefaultLanguageHighlighterColors.INVALID_STRING_ESCAPE);

    private static final TextAttributesKey[] EMPTY_KEYS = new TextAttributesKey[0];
    private static final TextAttributesKey[] KEYWORD_KEYS = new TextAttributesKey[]{KEYWORD};
    private static final TextAttributesKey[] COMMENT_KEYS = new TextAttributesKey[]{COMMENT};
    private static final TextAttributesKey[] STRING_KEYS = new TextAttributesKey[]{STRING};
    private static final TextAttributesKey[] DOCSTRING_KEYS = new TextAttributesKey[]{DOCSTRING};
    private static final TextAttributesKey[] OPERATOR_KEYS = new TextAttributesKey[]{OPERATOR};
    private static final TextAttributesKey[] NUMBER_KEYS = new TextAttributesKey[]{NUMBER};
    private static final TextAttributesKey[] EFFECT_NAME_KEYS = new TextAttributesKey[]{EFFECT_NAME};
    private static final TextAttributesKey[] IDENTIFIER_KEYS = new TextAttributesKey[]{IDENTIFIER};
    private static final TextAttributesKey[] INTERPOLATION_KEYS = new TextAttributesKey[]{INTERPOLATION};
    private static final TextAttributesKey[] REGEX_KEYS = new TextAttributesKey[]{REGEX};
    private static final TextAttributesKey[] MODULE_PATH_KEYS = new TextAttributesKey[]{MODULE_PATH};
    private static final TextAttributesKey[] BRACKETS_KEYS = new TextAttributesKey[]{BRACKETS};
    private static final TextAttributesKey[] BRACES_KEYS = new TextAttributesKey[]{BRACES};
    private static final TextAttributesKey[] PARENTHESES_KEYS = new TextAttributesKey[]{PARENTHESES};
    private static final TextAttributesKey[] COMMA_KEYS = new TextAttributesKey[]{COMMA};
    private static final TextAttributesKey[] BAD_CHAR_KEYS = new TextAttributesKey[]{BAD_CHARACTER};

    @NotNull
    @Override
    public Lexer getHighlightingLexer() {
        return new ReluxLexerAdapter();
    }

    @NotNull
    @Override
    public TextAttributesKey[] getTokenHighlights(IElementType tokenType) {
        // Keywords
        if (tokenType.equals(ReluxTokenTypes.TEST) ||
            tokenType.equals(ReluxTokenTypes.EFFECT) ||
            tokenType.equals(ReluxTokenTypes.FN) ||
            tokenType.equals(ReluxTokenTypes.IMPORT) ||
            tokenType.equals(ReluxTokenTypes.SHELL) ||
            tokenType.equals(ReluxTokenTypes.LET) ||
            tokenType.equals(ReluxTokenTypes.NEED) ||
            tokenType.equals(ReluxTokenTypes.AS) ||
            tokenType.equals(ReluxTokenTypes.CLEANUP) ||
            tokenType.equals(ReluxTokenTypes.SKIP) ||
            tokenType.equals(ReluxTokenTypes.RUN) ||
            tokenType.equals(ReluxTokenTypes.FLAKY) ||
            tokenType.equals(ReluxTokenTypes.IF) ||
            tokenType.equals(ReluxTokenTypes.UNLESS)) {
            return KEYWORD_KEYS;
        }

        // Operators
        if (tokenType.equals(ReluxTokenTypes.OP_SEND) ||
            tokenType.equals(ReluxTokenTypes.OP_SEND_RAW) ||
            tokenType.equals(ReluxTokenTypes.OP_MATCH_REGEX) ||
            tokenType.equals(ReluxTokenTypes.OP_MATCH_LITERAL) ||
            tokenType.equals(ReluxTokenTypes.OP_NEG_MATCH_REGEX) ||
            tokenType.equals(ReluxTokenTypes.OP_NEG_MATCH_LITERAL) ||
            tokenType.equals(ReluxTokenTypes.OP_FAIL_REGEX) ||
            tokenType.equals(ReluxTokenTypes.OP_FAIL_LITERAL) ||
            tokenType.equals(ReluxTokenTypes.OP_ARROW) ||
            tokenType.equals(ReluxTokenTypes.OP_ASSIGN) ||
            tokenType.equals(ReluxTokenTypes.CONDITION_OP)) {
            return OPERATOR_KEYS;
        }

        // Comments
        if (tokenType.equals(ReluxTokenTypes.COMMENT)) {
            return COMMENT_KEYS;
        }

        // Strings
        if (tokenType.equals(ReluxTokenTypes.STRING)) {
            return STRING_KEYS;
        }

        // Docstrings
        if (tokenType.equals(ReluxTokenTypes.DOCSTRING)) {
            return DOCSTRING_KEYS;
        }

        // Numbers and timeout
        if (tokenType.equals(ReluxTokenTypes.NUMBER) ||
            tokenType.equals(ReluxTokenTypes.TIMEOUT)) {
            return NUMBER_KEYS;
        }

        // Effect identifiers
        if (tokenType.equals(ReluxTokenTypes.EFFECT_IDENTIFIER)) {
            return EFFECT_NAME_KEYS;
        }

        // Module paths
        if (tokenType.equals(ReluxTokenTypes.MODULE_PATH)) {
            return MODULE_PATH_KEYS;
        }

        // Interpolation and capture variables
        if (tokenType.equals(ReluxTokenTypes.INTERPOLATION) ||
            tokenType.equals(ReluxTokenTypes.CAPTURE_VAR)) {
            return INTERPOLATION_KEYS;
        }

        // Regex payload
        if (tokenType.equals(ReluxTokenTypes.REGEX_PAYLOAD)) {
            return REGEX_KEYS;
        }

        // Regular identifiers and environment variables
        if (tokenType.equals(ReluxTokenTypes.IDENTIFIER) ||
            tokenType.equals(ReluxTokenTypes.ENV_VAR) ||
            tokenType.equals(ReluxTokenTypes.CONDITION_VALUE)) {
            return IDENTIFIER_KEYS;
        }

        // Payload (default string-like)
        if (tokenType.equals(ReluxTokenTypes.PAYLOAD)) {
            return STRING_KEYS;
        }

        // Punctuation
        if (tokenType.equals(ReluxTokenTypes.LBRACKET) ||
            tokenType.equals(ReluxTokenTypes.RBRACKET)) {
            return BRACKETS_KEYS;
        }

        if (tokenType.equals(ReluxTokenTypes.LBRACE) ||
            tokenType.equals(ReluxTokenTypes.RBRACE)) {
            return BRACES_KEYS;
        }

        if (tokenType.equals(ReluxTokenTypes.LPAREN) ||
            tokenType.equals(ReluxTokenTypes.RPAREN)) {
            return PARENTHESES_KEYS;
        }

        if (tokenType.equals(ReluxTokenTypes.COMMA)) {
            return COMMA_KEYS;
        }

        // Bad character
        if (tokenType.equals(TokenType.BAD_CHARACTER)) {
            return BAD_CHAR_KEYS;
        }

        return EMPTY_KEYS;
    }
}
