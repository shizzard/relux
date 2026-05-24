# Editor Support

Relux ships syntax highlighting and language support plugins for VS Code (and Cursor / VSCodium / code-server) and IntelliJ-family IDEs.

## VS Code, Cursor, code-server, VSCodium

The extension is published to two registries and works wherever you install it from:

- **[Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=spawnlink-eu.relux)** - VS Code, Cursor.
- **[Open VSX](https://open-vsx.org/extension/spawnlink-eu/relux)** - VSCodium, code-server, Gitpod, air-gapped derivatives.

Install from the command line:

```bash
code --install-extension spawnlink-eu.relux
```

Or search for **Relux** in the Extensions sidebar of your editor.

### Features

- Syntax highlighting for keywords, operators, strings, regex patterns, timeouts, comments.
- Bracket matching, auto-closing, folding.
- String interpolation highlighting (`${var}`, `$1`).

### Source

`editors/vscode/` in [shizzard/relux](https://github.com/shizzard/relux). Contributions welcome - see `editors/vscode/CONTRIBUTING.md`.

## IntelliJ IDEA, RustRover, CLion, PyCharm, GoLand, WebStorm

The IntelliJ plugin is published to the **[JetBrains Marketplace](https://plugins.jetbrains.com/plugin/relux)**.

Install via **Settings -> Plugins -> Marketplace -> search "Relux"**.

### Source

`editors/intellij/` in [shizzard/relux](https://github.com/shizzard/relux).
