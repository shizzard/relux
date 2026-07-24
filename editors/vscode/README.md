# Relux for VS Code

Syntax highlighting and language support for the [Relux](https://github.com/shizzard/relux)
DSL - Expect-style integration tests for interactive shell programs.

## Features

- Keyword highlighting (`test`, `effect`, `fn`, `shell`, `let`, `need`, `import`, `cleanup`)
- Shell operators (`>`, `=>`, `<?`, `<=`, `!?`, `!=`)
- String and doc-string highlighting with interpolation (`${var}`, `$1`)
- Regex pattern highlighting for match/fail operators
- Timeout duration highlighting (`~5s`, `~2h 30m`)
- Comment highlighting (`// ...`)
- Bracket matching, auto-closing, folding

## Install

Search **Relux** in the Extensions sidebar, or from the command line:

```sh
code --install-extension spawnlink-eu.relux
```

Also available on [Open VSX](https://open-vsx.org/extension/spawnlink-eu/relux) for VSCodium, Cursor, code-server, and other VS Code derivatives.

## Learn more

- [Project home](https://github.com/shizzard/relux)
- [DSL tutorial](https://shizzard.github.io/relux/latest/dsl-tutorial/)
- [Suite tutorial](https://shizzard.github.io/relux/latest/suite-tutorial/)
- [Reference](https://shizzard.github.io/relux/latest/reference/)

## License

MIT - see [LICENSE](LICENSE).
