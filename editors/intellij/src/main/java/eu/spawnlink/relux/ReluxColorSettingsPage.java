package eu.spawnlink.relux;

import com.intellij.openapi.editor.colors.TextAttributesKey;
import com.intellij.openapi.fileTypes.SyntaxHighlighter;
import com.intellij.openapi.options.colors.AttributesDescriptor;
import com.intellij.openapi.options.colors.ColorDescriptor;
import com.intellij.openapi.options.colors.ColorSettingsPage;
import org.jetbrains.annotations.NotNull;
import org.jetbrains.annotations.Nullable;

import javax.swing.*;
import java.util.Map;

/**
 * Color settings page for Relux language.
 * Allows users to customize syntax highlighting colors.
 */
public class ReluxColorSettingsPage implements ColorSettingsPage {
    private static final AttributesDescriptor[] DESCRIPTORS = new AttributesDescriptor[]{
            new AttributesDescriptor("Keyword", ReluxSyntaxHighlighter.KEYWORD),
            new AttributesDescriptor("Comment", ReluxSyntaxHighlighter.COMMENT),
            new AttributesDescriptor("String", ReluxSyntaxHighlighter.STRING),
            new AttributesDescriptor("Docstring", ReluxSyntaxHighlighter.DOCSTRING),
            new AttributesDescriptor("Operator", ReluxSyntaxHighlighter.OPERATOR),
            new AttributesDescriptor("Number//Timeout", ReluxSyntaxHighlighter.NUMBER),
            new AttributesDescriptor("Effect Name", ReluxSyntaxHighlighter.EFFECT_NAME),
            new AttributesDescriptor("Identifier", ReluxSyntaxHighlighter.IDENTIFIER),
            new AttributesDescriptor("Interpolation", ReluxSyntaxHighlighter.INTERPOLATION),
            new AttributesDescriptor("Regex Pattern", ReluxSyntaxHighlighter.REGEX),
            new AttributesDescriptor("Module Path", ReluxSyntaxHighlighter.MODULE_PATH),
            new AttributesDescriptor("Brackets", ReluxSyntaxHighlighter.BRACKETS),
            new AttributesDescriptor("Braces", ReluxSyntaxHighlighter.BRACES),
            new AttributesDescriptor("Parentheses", ReluxSyntaxHighlighter.PARENTHESES),
            new AttributesDescriptor("Comma", ReluxSyntaxHighlighter.COMMA),
            new AttributesDescriptor("Bad Character", ReluxSyntaxHighlighter.BAD_CHARACTER)
    };

    @Nullable
    @Override
    public Icon getIcon() {
        return ReluxIcons.FILE;
    }

    @NotNull
    @Override
    public SyntaxHighlighter getHighlighter() {
        return new ReluxSyntaxHighlighter();
    }

    @NotNull
    @Override
    public String getDemoText() {
        return """
                # Relux syntax demo
                import lib/module1 {
                    function1, function2, Effect1 as E1
                }

                fn example_function(arg1, arg2) {
                    > echo "arg: ${arg1}"
                    <? match regex here (\\d+)
                    let captured = ${1}
                    ${captured}
                }

                effect StartServer -> server {
                    need E1 as e1

                    shell server {
                        ~10s
                        !? [Ee]rror|FATAL
                        > ./start_server
                        <= Server started
                    }

                    cleanup {
                        > ./stop_server
                    }
                }

                [skip unless CI]
                test "Example test" {
                    \"""
                    Test description goes here
                    \"""

                    need StartServer as srv

                    shell myshell {
                        > curl localhost:8080
                        <? 200 OK

                        <~5s? quick response
                        <!? FATAL
                    }
                }
                """;
    }

    @Nullable
    @Override
    public Map<String, TextAttributesKey> getAdditionalHighlightingTagToDescriptorMap() {
        return null;
    }

    @NotNull
    @Override
    public AttributesDescriptor[] getAttributeDescriptors() {
        return DESCRIPTORS;
    }

    @NotNull
    @Override
    public ColorDescriptor[] getColorDescriptors() {
        return ColorDescriptor.EMPTY_ARRAY;
    }

    @NotNull
    @Override
    public String getDisplayName() {
        return "Relux";
    }
}
