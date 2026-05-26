---
name: relux:install
description: Install or update the relux binary. Use when the user asks to install relux, set up relux on a machine, get relux running for the first time, update relux, or upgrade to a newer published version. Also fires when the agent encounters a Relux.toml in a repository but `relux` is missing from PATH or its `--version` is older than the latest crates.io / GitHub Releases tag.
---

# Install or update relux

Bring the `relux` binary to a usable, up-to-date state. This skill handles
first-time install, re-install, and upgrade through the same workflow. The
canonical install path is `cargo install relux`; pre-built tarballs and a
from-source build are documented branches for when cargo does not fit.

## When to use

User phrasings:

- "Install relux."
- "Set up relux on this machine."
- "How do I get relux running?"
- "Update relux." / "Upgrade relux to the latest version."

Agent-task signals:

- A repository contains `Relux.toml` but `command -v relux` fails.
- `relux --version` reports a version older than the latest published release.
- The user is about to run `relux check` or `relux run` for the first time.

**Direct invocation (`/relux:install`).** No clarification needed; the
workflow always runs against the local host. Proceed straight to
pre-flight checks.

## Pre-flight checks

- [ ] `relux --version` -- captures the installed version, or confirms absence.
- [ ] `command -v cargo && cargo --version` -- determines whether the cargo
      path is available.
- [ ] `uname -sm` -- platform string for selecting the right pre-built asset.
- [ ] `echo "$SHELL"` -- needed for the completion install path.
- [ ] Latest published version probe (pick one):
      - `gh release view --repo shizzard/relux --json tagName -q .tagName`
      - `curl -sL https://crates.io/api/v1/crates/relux | jq -r .crate.max_version`

## Workflow

### 1. Determine state

Compare the installed version against the latest published version and pick a
branch:

- **Not installed** -- go to step 2.
- **Installed and at latest** -- skip to step 5 (completions) and step 6
  (follow-ups).
- **Installed and outdated** -- go to step 3.

### 2. Install

Pick the install method in this order. Stop at the first that fits.

**Cargo (default).** Universal; requires Rust 1.85 or newer.

```bash
rustc --version           # confirm >= 1.85
cargo install relux
```

If `rustc --version` reports a version older than 1.85, **ask the user
before upgrading the toolchain**; `rustup update stable` rewrites their
default channel and may pull in clippy / rustfmt / target updates they
weren't expecting.

```bash
# Confirm with the user before running this.
rustup update stable
```

If `cargo` is absent, stop and instruct the user to install Rust via
[rustup](https://rustup.rs/). Also surface the pre-built branch as the
zero-toolchain workaround so the user can choose without leaving the
conversation.

**Pre-built tarball.** No Rust toolchain needed. Linux x86_64 and macOS
aarch64 only.

- Releases page: <https://github.com/shizzard/relux/releases/latest>.
- Pick the asset target triple by `uname -sm`:

  | `uname -sm`     | Asset target triple        |
  |-----------------|----------------------------|
  | `Darwin arm64`  | `aarch64-apple-darwin`     |
  | `Linux x86_64`  | `x86_64-unknown-linux-gnu` |

- Download, extract, place on PATH:

  ```bash
  TAG="$(gh release view --repo shizzard/relux --json tagName -q .tagName)"
  TARGET="aarch64-apple-darwin"            # pick per the table above
  ASSET="relux-${TAG}-${TARGET}.tar.gz"
  gh release download "${TAG}" --repo shizzard/relux --pattern "${ASSET}"
  tar -xzf "${ASSET}"
  chmod +x relux
  mv relux ~/.local/bin/                    # or any PATH directory
  ```

**From source.** Use when developing on relux itself, or when neither cargo
nor a pre-built target fits. The vendored viewer bundle at
`vendor/relux-viewer.js.gz` is committed and embedded at compile time, so a
bare cargo build produces a fully functional binary.

```bash
git clone git@github.com:shizzard/relux.git
cd relux
cargo build --release
# Symlink (tracks future rebuilds) -- preferred for active development:
ln -s "$(pwd)/target/release/relux" ~/.local/bin/relux
# Or copy (one-shot snapshot, won't track rebuilds):
# cp target/release/relux ~/.local/bin/
```

### 3. Update (installed but outdated)

Identify the original install path so the upgrade command matches:

```bash
which relux                     # ~/.cargo/bin/relux  -> cargo
                                # ~/.local/bin/relux  -> pre-built or source
readlink "$(which relux)"       # if a symlink, source is a from-source build
```

Pick the upgrade command:

- **Cargo:** `cargo install relux --force` (without `--force`, cargo silently
  no-ops when the binary already exists).
- **Pre-built:** repeat step 2's pre-built branch with the new `TAG`.
- **From-source symlink:** `(cd <checkout> && git pull && cargo build --release)`
  -- the symlink picks up the new binary automatically.
- **From-source copy:** rebuild with `cargo build --release` from the
  checkout and re-copy.

Before triggering the upgrade, surface what changed.

```bash
# Single-release gap -- render release notes inline:
gh release view "${NEW_TAG}" --repo shizzard/relux

# Multi-release gap -- link the compare view and stop:
echo "https://github.com/shizzard/relux/compare/${OLD_TAG}...${NEW_TAG}"
```

Render inline only when the gap spans one or two releases; for larger gaps,
hand the user the compare link and let them browse.

### 4. Verify

```bash
relux --version
```

The reported version must match what was just installed or upgraded. If the
command is not found, the install location is not on PATH; inspect the user's
shell rc and add the relevant `bin` directory.

### 5. Shell completions

Ask the user whether they want shell completions installed for `relux`. Do
not install them unprompted -- completions write to the user's shell config
or completion directories, which is a touch outside the install itself.
Skip the prompt on a no-op same-version verification.

On consent, install for the user's shell:

```bash
relux completions --install       # bash, fish: autodetect
```

For zsh, an explicit path is required:

```bash
mkdir -p ~/.zsh/completions
relux completions --shell zsh --install --path ~/.zsh/completions
# Then ensure ~/.zshrc has:
#   fpath=(~/.zsh/completions $fpath)
#   autoload -U compinit && compinit
```

### 6. Follow-ups

- Ask the user whether they want to install a `.relux` editor plugin for
  VS Code or IntelliJ. On consent, invoke the `relux:editor-plugin`
  skill via the Skill tool. Do not invoke unprompted; the user may already
  have it installed or may not use a supported editor. Skip the prompt
  entirely on a no-op same-version verification.
- If no `Relux.toml` exists on the current directory's ancestor chain,
  ask the user whether they want to bootstrap a suite here. On consent,
  invoke the `relux:init` skill via the Skill tool. Skip the prompt
  if a suite already exists (the user is upgrading inside a known
  project) or on a no-op same-version verification.
- Point the user at the tutorials matching the installed version:
  - Latest: <https://shizzard.github.io/relux/latest/>
  - Specific version: `https://shizzard.github.io/relux/v<X.Y.Z>/`

## Done when

- `relux --version` succeeds in a fresh shell and prints the expected version.
- The user has been offered shell completions (and either declined or had
  them installed on consent).
- The user has been offered the `relux:editor-plugin` follow-up
  (and either declined or had the skill invoked on consent).
- If no `Relux.toml` exists on the cwd's ancestor chain, the user has
  been offered the `relux:init` follow-up (and either declined or had
  the skill invoked on consent).
- The user has been pointed at the tutorials URL matching their
  installed version.

## Cross-skill handoffs

- `relux:editor-plugin` -- offered after step 5 on a fresh install
  or upgrade, gated on user consent (see step 6).
- `relux:init` -- offered in step 6 when the cwd has no `Relux.toml`
  on its ancestor chain, gated on user consent.

## References

- `references/cli-reference.md` -- the subcommands the binary now exposes.

## Pitfalls

### Cargo bin not on PATH

A fresh `cargo install` places the binary in `~/.cargo/bin/`. If that
directory is not on the shell's PATH, `relux --version` fails immediately
after install.

Don't:

```bash
cargo install relux
relux --version       # command not found
```

Do:

```bash
cargo install relux
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc      # or ~/.bashrc
exec "$SHELL" -l
relux --version
```

### Cargo silently skips upgrades without `--force`

`cargo install <crate>` on a host that already has the binary is a no-op,
even when a newer version exists on crates.io.

Don't:

```bash
cargo install relux             # no upgrade; installed version unchanged
```

Do:

```bash
cargo install relux --force     # forces the upgrade
relux --version                 # verify the new version
```

### zsh completions need an explicit path

bash and fish autodetect; zsh does not. Skipping `--path` is silently a
no-op under zsh.

Don't:

```bash
relux completions --install     # silent no-op under zsh
```

Do:

```bash
mkdir -p ~/.zsh/completions
relux completions --shell zsh --install --path ~/.zsh/completions
```

### Active development: symlink, don't copy

When working on relux itself, a copied binary drifts from the checkout on
every rebuild. A symlink tracks `target/release/relux` for free.

Don't:

```bash
cp target/release/relux ~/.local/bin/    # next rebuild is invisible
```

Do:

```bash
ln -s "$(pwd)/target/release/relux" ~/.local/bin/relux
```