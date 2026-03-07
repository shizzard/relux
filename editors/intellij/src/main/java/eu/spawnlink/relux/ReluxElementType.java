package eu.spawnlink.relux;

import com.intellij.psi.tree.IElementType;
import org.jetbrains.annotations.NonNls;
import org.jetbrains.annotations.NotNull;

/**
 * Element type for Relux tokens.
 */
public class ReluxElementType extends IElementType {
    public ReluxElementType(@NotNull @NonNls String debugName) {
        super(debugName, ReluxLanguage.INSTANCE);
    }
}
