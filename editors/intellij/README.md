# Relux for IntelliJ IDEA

Syntax highlighting and editor support for the [Relux](https://github.com/spawnlink/relux) DSL (`.relux` files).

## Features

- **Syntax Highlighting**: Keywords, operators, strings, comments, and special constructs
- **Smart Editor Features**:
  - Bracket, brace, and parenthesis matching
  - Line comment support (`# ...`)
  - Code folding for `test`, `effect`, `fn`, `shell`, and `cleanup` blocks
- **Customizable Colors**: Configure syntax colors via Settings → Editor → Color Scheme → Relux

## Supported Syntax

- Keywords: `test`, `effect`, `fn`, `import`, `shell`, `let`, `need`, `as`, `cleanup`
- Shell operators: `>`, `=>`, `<?`, `<=`, `<!?`, `<!=`, `!?`, `!=`
- Timed operators: `<~5s?`, `<~10s=`, etc.
- Condition markers: `[skip|run|flaky if|unless CONDITION]`
- String interpolation: `${var}`, `$0`, `$1`, etc.
- Comments and docstrings

## Installation

### From Source

1. Build the plugin (from the project root):
   ```bash
   just intellij
   ```
   > **Note:** The first build downloads the full IntelliJ IDEA SDK (~1.5 GB). Subsequent builds use the Gradle cache.

2. Install the plugin:
   - Open IntelliJ IDEA
   - Go to Settings → Plugins → ⚙️ → Install Plugin from Disk...
   - Select `build/distributions/relux-intellij-plugin-0.1.0.zip`
   - Restart the IDE

### Local Development

1. Open the `editors/intellij` directory in IntelliJ IDEA
2. Run the Gradle task `runIde` to launch a development instance with the plugin installed
3. Make changes and test in the development instance

## Building

```bash
# Generate lexer from JFlex specification
./gradlew generateLexer

# Build plugin
./gradlew buildPlugin

# Run IDE with plugin for testing
./gradlew runIde
```

## Requirements

- IntelliJ IDEA 2024.3 or later
- Java 17 or later

## License

MIT
