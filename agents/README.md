# Relux Agent Plugin

Skills for authoring, running, and diagnosing [Relux](https://github.com/shizzard/relux) test suites.

v1 targets Claude Code. The directory layout is positioned so manifests for Cursor, OpenCode, Gemini CLI, and Codex can be added later without restructuring the portable content.

## Install (Claude Code)

```
claude plugin install <path-to-this-repo>/agents
```

Skills surface as `/relux:<skill-name>` slash commands and as auto-triggered behaviour driven by each skill's `description:` field.

## How this is structured

- `.claude-plugin/plugin.json` -- Claude Code manifest (name, description, author, version).
- `skills/<name>/SKILL.md` -- one directory per skill. Portable Markdown; no Claude-specific syntax aside from tool-name references (Read, Edit, Bash) and cross-skill handoff sentences.
- `references/*.md` -- shared content loaded on demand by skills. Not skills themselves; they have no `description:` and never auto-trigger.
- `AGENTS.md` -- cross-agent guidance (placeholder; v1 targets Claude Code only).
- `LICENSE` -- MIT, matching the repo.

Validation runs via `just build-plugin` from the repo root; it is folded into `just check` and the pre-commit hook.

## Versioning

The plugin's `version` field tracks the Relux workspace version, bumped by release-please on tagged releases.
