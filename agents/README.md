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

Validation runs via `just build-agents` from the repo root; it is folded into `just build` and the pre-commit hook.

## Authoring rule: domain knowledge in references, skills are about logic

Skills are loaded into the agent's context every time they are invoked; references load on demand. Inlining reference material into a skill costs context on every invocation and risks drift between the skill copy and the canonical source.

**The rule.**

- **References** own *domain knowledge*: syntactic forms, semantic rules, lifecycle, evaluation order, identity tuples, slot scopes, expose semantics, cleanup discipline, marker forms, BIF lists -- anything that describes *how the language and runtime behave*.
- **Skills** own *logic*: when to invoke, decomposition / path / checklist structure, blast-radius analysis, sequencing of operations, judgement calls under ambiguity, verification rhythm -- anything that describes *what the agent should decide and in what order*.

**When a skill needs a domain rule:** point at the reference (`references/<file>.md` > *<section>*) and apply the rule. Do not restate the rule in the skill body. A required pre-flight read of the reference carries the load; the skill text says *which* rule applies *where in the workflow*, not what the rule says.

**Authoring loop when slimming or extending a skill:**

1. Before inlining any domain content, locate the reference that owns it.
2. If the reference covers it, point at the reference -- do not restate.
3. If the reference does not cover it, add the rule to the reference first (with its canonical Don't/Do example), then point at it from the skill.
4. Keep skill-specific application notes (one line at the point of use) only when the workflow's *use* of the rule is non-obvious; otherwise the pointer alone is enough.

A skill that reads as "here is the workflow; here is the discipline; reference X owns the semantics" is doing it right. A skill that reads as a paraphrase of a reference is doing it wrong.

## Versioning

The plugin's `version` field tracks the Relux workspace version, bumped by release-please on tagged releases.
