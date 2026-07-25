---
name: relux:editor-plugin
description: Install or update the relux editor plugin for VS Code (and Cursor / VSCodium / code-server) or JetBrains-family IDEs (IntelliJ IDEA, RustRover, CLion, PyCharm, GoLand, WebStorm). Use when the user asks for syntax highlighting on .relux files, wants to set up their editor for relux work, or upgrades an outdated plugin. Often invoked as a follow-up by relux:install after the binary is on PATH.
---

# Install or update the relux editor plugin

Bring the relux editor plugin to a usable, up-to-date state for the user's
editor of choice. The plugin provides syntax highlighting and bracket /
folding support for `.relux` files; it does not yet ship a language
server.

The plugin is published to three registries; pick the one that matches the
editor:

- **Visual Studio Marketplace** -- VS Code, Cursor.
- **Open VSX** -- VSCodium, code-server, Gitpod, air-gapped derivatives.
- **JetBrains Marketplace** -- IntelliJ IDEA, RustRover, CLion, PyCharm,
  GoLand, WebStorm, etc.

GitHub Releases also publishes `.vsix` and `.zip` artifacts as a fallback
for offline / version-pinned installs.

## When to use

User phrasings:

- "Install the VS Code plugin for relux."
- "Install the IntelliJ plugin for relux."
- "Set up Cursor / VSCodium / code-server for relux."
- "Why isn't my `.relux` file highlighted?"
- "Update the relux editor plugin."

Agent-task signals:

- Invoked as a follow-up from `relux:install` step 6, on user consent.
- A `.relux` file is open in the conversation and the user remarks on
  missing or stale highlighting.

**Direct invocation (`/relux:editor-plugin`).** Ask which editor family
the user is targeting (VS Code / Cursor / VSCodium / code-server /
Gitpod / a JetBrains IDE) before starting -- the registry and install
command depend on it. If the user picks a derivative (Cursor,
VSCodium), name the underlying registry (Marketplace vs Open VSX) in
the confirmation so they understand which catalog the plugin is
coming from.

## Pre-flight checks

- [ ] Confirm target editor *family* (ask if ambiguous; both is fine if the
      user uses both). Note specifically: VS Code vs Cursor vs VSCodium vs
      code-server -- they share a `code`-style CLI but pull from
      different registries.
- [ ] For VS Code-family: `code --version` (or `codium --version`,
      `cursor --version`) -- confirms the CLI is on PATH.
- [ ] For VS Code-family: `code --list-extensions --show-versions | grep -i relux`
      -- captures installed version, or confirms absence.
- [ ] For JetBrains: probing requires the GUI; ask the user what they see
      under Settings -> Plugins -> Installed.
- [ ] Latest published version probe:
      `gh release view --repo shizzard/relux --json tagName -q .tagName`.

## Workflow

### 1. Pick editor and registry

Map editor to registry. The CLI name (`code` / `codium` / `cursor` /
`code-server`) is incidental; what matters is which registry the editor
talks to:

| Editor                        | CLI            | Registry                |
|-------------------------------|----------------|-------------------------|
| VS Code, Cursor               | `code`         | Visual Studio Marketplace |
| VSCodium, code-server, Gitpod | `codium`, etc. | Open VSX                |
| IntelliJ IDEA, RustRover, ... | (GUI)          | JetBrains Marketplace   |

If the user uses several editors, run the workflow once per editor.

### 2. Determine state

Compare the installed plugin version against the latest published version:

- **Not installed** -- go to step 3.
- **Installed at latest** -- go to step 5 (verify).
- **Installed but outdated** -- go to step 4.

### 3. Install

**VS Code / Cursor via Visual Studio Marketplace.** Canonical path.

```bash
code --install-extension spawnlink-eu.relux
# or, for Cursor:
cursor --install-extension spawnlink-eu.relux
```

**VSCodium / code-server / Gitpod via Open VSX.** Canonical path -- these
editors do not ship with access to the Visual Studio Marketplace.

```bash
codium --install-extension spawnlink-eu.relux
# or, for code-server:
code-server --install-extension spawnlink-eu.relux
```

For Gitpod, install via the Extensions sidebar (search "Relux"); the
workspace pulls from Open VSX automatically.

**JetBrains via Marketplace.** Canonical path.

1. Open Settings (Cmd/Ctrl+,).
2. Plugins -> Marketplace -> search "Relux".
3. Install -> Restart IDE when prompted.

**VS Code-family via VSIX from GitHub Releases.** Fallback for offline,
air-gapped, or version-pinned installs.

```bash
TAG="$(gh release view --repo shizzard/relux --json tagName -q .tagName)"
ASSET="relux-vscode-${TAG}.vsix"
gh release download "${TAG}" --repo shizzard/relux --pattern "${ASSET}"
code --install-extension "${ASSET}"
```

**JetBrains via ZIP from GitHub Releases.** Fallback when the IDE cannot
reach the JetBrains Marketplace, or when pinning a specific version.

```bash
TAG="$(gh release view --repo shizzard/relux --json tagName -q .tagName)"
ASSET="relux-intellij-${TAG}.zip"
gh release download "${TAG}" --repo shizzard/relux --pattern "${ASSET}"
```

Then in the IDE: Settings -> Plugins -> gear icon (top right of the panel)
-> "Install Plugin from Disk..." -> select the downloaded `.zip` -> Restart.

**From source.** Use when developing on relux itself, or when neither
registry nor pre-built artifact fits.

VS Code-family, via the just recipe:

```bash
git clone git@github.com:shizzard/relux.git
cd relux
just build-vscode
code --install-extension editors/vscode/build/relux.vsix
```

VS Code-family, when `just` is not installed: read the `build-vscode`
recipe in `justfile` and run the commands it lists. The recipe is the
source of truth; the snippet below is illustrative and may drift.

The recipe runs `vsce` inside `node:lts-slim` via docker so nothing is
installed on the host:

```bash
cd relux/editors/vscode
mkdir -p build
docker run --rm -v "$PWD":/src -w /src node:lts-slim \
    sh -c 'npx --yes @vscode/vsce package --out /src/build/relux.vsix'
code --install-extension build/relux.vsix
```

If docker is unavailable, fall back to a host install of `vsce` -- **but
ask the user first**, since `npm install -g` writes into the system /
user-level npm prefix:

```bash
# Confirm with the user before running this.
npm install -g @vscode/vsce
cd relux/editors/vscode
vsce package --out build/relux.vsix
code --install-extension build/relux.vsix
```

JetBrains, via the just recipe:

```bash
git clone git@github.com:shizzard/relux.git
cd relux
just build-intellij
# Output: editors/intellij/build/distributions/relux-<version>.zip
# Install via IDE: Settings -> Plugins -> gear -> Install Plugin from Disk.
```

JetBrains, when `just` is not installed: read the `build-intellij` recipe
in `justfile` and run the commands it lists. The recipe is the source of
truth; the snippet below is illustrative and may drift.

```bash
cd relux/editors/intellij
gradle buildPlugin
# Same install-from-disk step as above.
```

### 4. Update (installed but outdated)

Before triggering, surface what changed.

```bash
# Single-release gap -- render release notes inline:
gh release view "${NEW_TAG}" --repo shizzard/relux

# Multi-release gap -- link the compare view and stop:
echo "https://github.com/shizzard/relux/compare/${OLD_TAG}...${NEW_TAG}"
```

Render inline only when the gap spans one or two releases; for larger gaps,
hand the user the compare link and let them browse.

Pick the upgrade path matching how the plugin was originally installed.
Do not silently switch install method on upgrade; if the user wants to
migrate (e.g. VSIX -> Marketplace), ask first and have them uninstall the
old install first.

- **VS Marketplace or Open VSX:** auto-updates by default. If the user
  disabled auto-update, force the update:

  ```bash
  code --install-extension spawnlink-eu.relux --force
  ```

- **VSIX from GitHub Releases:** re-download and re-install with `--force`
  (without it, `code --install-extension` is a no-op on the same major
  version).

  ```bash
  gh release download "${NEW_TAG}" --repo shizzard/relux \
      --pattern "relux-vscode-${NEW_TAG}.vsix"
  code --install-extension "relux-vscode-${NEW_TAG}.vsix" --force
  ```

- **JetBrains Marketplace:** Settings -> Plugins -> Updates tab. The IDE
  shows a banner when an update is available; one click upgrades it.

- **JetBrains ZIP-from-disk:** re-download the new `.zip` and re-install
  from disk; the IDE replaces the old version.

- **From source:** `git pull` the checkout, rerun the build recipe from
  step 3, re-install.

### 5. Verify

- Open any `.relux` file in the editor.
- Confirm syntax highlighting: keywords (`test`, `effect`, `let`, `send`,
  `expect`), identifiers, and strings render distinctly.
- For VS Code-family, the installed version can be checked from the
  command line:

  ```bash
  code --list-extensions --show-versions | grep -i relux
  ```

- For JetBrains, check Settings -> Plugins -> Installed -> Relux.

If highlighting is missing immediately after install, the editor usually
needs a restart for the language contribution to register.

## Done when

- The plugin is installed (or upgraded) in the user's editor.
- The user has opened a `.relux` file and confirmed syntax highlighting
  renders.

## Cross-skill handoffs

- Invoked from `relux:install` step 6 on user consent. No further handoff
  out of this skill.

## References

None. The workflow is self-contained; install routes are linked inline.

## Pitfalls

### macOS: the `code` CLI is not on PATH by default

The `code` CLI must be enabled separately from inside VS Code, otherwise
`code --install-extension` fails with "command not found". The same goes
for Cursor's `cursor` CLI.

Don't:

```bash
code --install-extension spawnlink-eu.relux       # command not found
```

Do:

1. Open VS Code (or Cursor).
2. Cmd+Shift+P -> "Shell Command: Install 'code' command in PATH"
   (Cursor: "Install 'cursor' command in PATH").
3. Retry the install in a fresh terminal.

### Wrong registry for the editor

VS Marketplace and Open VSX are separate registries. VSCodium and
code-server cannot reach VS Marketplace -- their `--install-extension`
calls hit Open VSX. Picking the wrong registry yields an "extension not
found" error even though the plugin exists.

Don't:

- Tell a VSCodium user to install from VS Marketplace.
- Tell a VS Code user to install from Open VSX (it works, but the
  Marketplace listing is the better-maintained source).

Do:

- VS Code, Cursor -> VS Marketplace.
- VSCodium, code-server, Gitpod -> Open VSX.

Either case, the extension ID is the same: `spawnlink-eu.relux`.

### `code --install-extension` is a no-op without `--force`

Just like `cargo install`, re-installing an extension that is already
present is a silent no-op unless `--force` is passed. Upgrades fail
quietly when going through the CLI.

Don't:

```bash
code --install-extension relux-vscode-0.2.0.vsix    # no-op if 0.1.x present
```

Do:

```bash
code --install-extension relux-vscode-0.2.0.vsix --force
code --list-extensions --show-versions | grep -i relux    # verify
```

### JetBrains: "Install from Disk" hides under the gear menu

The search bar in Settings -> Plugins searches the JetBrains Marketplace;
it cannot open a local `.zip`. The "Install Plugin from Disk..." action
lives under the gear icon at the top right of the plugins panel. Users
often miss it and conclude the disk install path is broken.

### JetBrains plugins install per IDE, not globally

A plugin installed in IntelliJ IDEA is not visible in PyCharm or GoLand on
the same machine; each IDE keeps its own plugins directory. If the user
uses several JetBrains IDEs, repeat the install in each.
