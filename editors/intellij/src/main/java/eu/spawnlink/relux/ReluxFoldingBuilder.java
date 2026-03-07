package eu.spawnlink.relux;

import com.intellij.lang.ASTNode;
import com.intellij.lang.folding.FoldingBuilderEx;
import com.intellij.lang.folding.FoldingDescriptor;
import com.intellij.openapi.editor.Document;
import com.intellij.openapi.util.TextRange;
import com.intellij.psi.PsiElement;
import com.intellij.psi.util.PsiTreeUtil;
import org.jetbrains.annotations.NotNull;
import org.jetbrains.annotations.Nullable;

import java.util.ArrayList;
import java.util.List;

/**
 * Folding builder for Relux language blocks.
 */
public class ReluxFoldingBuilder extends FoldingBuilderEx {
    @NotNull
    @Override
    public FoldingDescriptor[] buildFoldRegions(@NotNull PsiElement root, @NotNull Document document, boolean quick) {
        List<FoldingDescriptor> descriptors = new ArrayList<>();

        // Collect all brace pairs for folding
        collectFoldingRegions(root, descriptors, document);

        return descriptors.toArray(new FoldingDescriptor[0]);
    }

    private void collectFoldingRegions(PsiElement element, List<FoldingDescriptor> descriptors, Document document) {
        for (PsiElement child : element.getChildren()) {
            ASTNode node = child.getNode();
            if (node != null && node.getElementType() == ReluxTokenTypes.LBRACE) {
                // Find matching closing brace
                PsiElement closingBrace = findMatchingBrace(child.getParent());
                if (closingBrace != null) {
                    TextRange range = new TextRange(
                            child.getTextRange().getStartOffset(),
                            closingBrace.getTextRange().getEndOffset()
                    );
                    if (range.getLength() > 2) { // Only fold non-empty blocks
                        descriptors.add(new FoldingDescriptor(node, range));
                    }
                }
            }

            // Recursively collect from children
            collectFoldingRegions(child, descriptors, document);
        }
    }

    private PsiElement findMatchingBrace(PsiElement parent) {
        if (parent == null) return null;

        int depth = 0;
        for (PsiElement child : parent.getChildren()) {
            ASTNode node = child.getNode();
            if (node != null) {
                if (node.getElementType() == ReluxTokenTypes.LBRACE) {
                    depth++;
                } else if (node.getElementType() == ReluxTokenTypes.RBRACE) {
                    depth--;
                    if (depth == 0) {
                        return child;
                    }
                }
            }
        }
        return null;
    }

    @Nullable
    @Override
    public String getPlaceholderText(@NotNull ASTNode node) {
        return "{...}";
    }

    @Override
    public boolean isCollapsedByDefault(@NotNull ASTNode node) {
        return false;
    }
}
