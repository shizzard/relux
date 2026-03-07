package eu.spawnlink.relux;

import com.intellij.lang.Language;

/**
 * Relux language definition.
 */
public class ReluxLanguage extends Language {
    public static final ReluxLanguage INSTANCE = new ReluxLanguage();

    private ReluxLanguage() {
        super("Relux");
    }
}
