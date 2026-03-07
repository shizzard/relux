package eu.spawnlink.relux;

import com.intellij.lexer.FlexAdapter;

/**
 * Adapter for the JFlex-generated Relux lexer.
 */
public class ReluxLexerAdapter extends FlexAdapter {
    public ReluxLexerAdapter() {
        super(new ReluxLexer(null));
    }
}
