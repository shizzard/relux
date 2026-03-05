# Relux for VS Code / Cursor

Syntax highlighting for the [Relux](https://github.com/spawnlink/relux) DSL (`.relux` files).

## Features

- Keyword highlighting (`test`, `effect`, `fn`, `shell`, `let`, `need`, `import`, `cleanup`)
- Shell operators (`>`, `=>`, `<?`, `<=`, `!?`, `!=`)
- String and doc-string highlighting with interpolation support (`${var}`, `$1`)
- Regex pattern highlighting for match/fail operators
- Timeout duration highlighting (`~5s`, `~2h 30m`)
- Comment highlighting (`# ...`)
- Bracket matching, auto-closing, and folding

## Local installation

Symlink this directory into your editor's extensions folder:

```bash
# Cursor
ln -s "$(pwd)" ~/.cursor/extensions/relux

# VS Code
ln -s "$(pwd)" ~/.vscode/extensions/relux
```

Then reload the editor window (Cmd+Shift+P -> "Developer: Reload Window").

## Packaging

To build a `.vsix` for distribution:

```bash
npm install -g @vscode/vsce
vsce package
```
