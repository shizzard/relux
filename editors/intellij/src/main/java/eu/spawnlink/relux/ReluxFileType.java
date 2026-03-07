package eu.spawnlink.relux;

import com.intellij.openapi.fileTypes.LanguageFileType;
import org.jetbrains.annotations.NotNull;

import javax.swing.*;

/**
 * Relux file type definition.
 */
public class ReluxFileType extends LanguageFileType {
    public static final ReluxFileType INSTANCE = new ReluxFileType();

    private ReluxFileType() {
        super(ReluxLanguage.INSTANCE);
    }

    @NotNull
    @Override
    public String getName() {
        return "Relux";
    }

    @NotNull
    @Override
    public String getDescription() {
        return "Relux test automation file";
    }

    @NotNull
    @Override
    public String getDefaultExtension() {
        return "relux";
    }

    @Override
    public Icon getIcon() {
        return ReluxIcons.FILE;
    }
}
