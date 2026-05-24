# Contributing to the Relux VS Code extension

This document covers developer / maintainer workflows. End users should not need it.

## Local installation (sideload from source)

Symlink this directory into your editor's extensions folder:

```bash
# Cursor
ln -s "$(pwd)" ~/.cursor/extensions/relux

# VS Code
ln -s "$(pwd)" ~/.vscode/extensions/relux
```

Then reload the editor window (Cmd+Shift+P -> "Developer: Reload Window").

## Building a .vsix locally

```bash
just build-vscode
```

Output: `editors/vscode/build/relux.vsix`. Install with `code --install-extension build/relux.vsix`.

## Checking the manifest

```bash
just check-vscode
```

Runs `vsce package` against a throwaway path and fails on any `vsce` warning. Catches missing icons, broken URLs, missing license, etc., before they reach a tagged release.

## Versioning

The `version` field in `package.json` is owned by [release-please](https://github.com/googleapis/release-please) - it is bumped automatically on every release-please PR via the `extra-files` entry in `release-please-config.json`. Do not bump it manually.

## Publishing

Publishing to the VS Code Marketplace and Open VSX is CI-only - see `.github/workflows/release.yml` (`vscode-publish` job). There is intentionally no `just publish-vscode` target; local PATs would invite accidental publishes.

If you need to rotate credentials, see the operator runbook in [docs/superpowers/specs/2026-05-24-vscode-marketplace-publishing-design.md](../../docs/superpowers/specs/2026-05-24-vscode-marketplace-publishing-design.md).

## Icon

`icon.png` is a 128x128 PNG rasterized from `editors/intellij/src/main/resources/META-INF/pluginIcon.svg`. If the logo evolves, update both files in the same commit.

Any tool that produces a 128x128 PNG works. Some options:

```bash
# rsvg-convert (brew install librsvg)
rsvg-convert -w 128 -h 128 \
    editors/intellij/src/main/resources/META-INF/pluginIcon.svg \
    -o editors/vscode/icon.png

# macOS built-in (no extra deps)
qlmanage -t -s 128 -o /tmp/relux-icon \
    editors/intellij/src/main/resources/META-INF/pluginIcon.svg
cp /tmp/relux-icon/pluginIcon.svg.png editors/vscode/icon.png

# Inkscape
inkscape --export-type=png --export-width=128 --export-height=128 \
    --export-filename=editors/vscode/icon.png \
    editors/intellij/src/main/resources/META-INF/pluginIcon.svg
```

The SVG is the source of truth; the PNG is a build artifact committed for convenience.
