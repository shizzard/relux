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
- `tools/*` -- small executables that skills can shell out to. Python 3 (stdlib only) and POSIX shell only -- see *Tools policy* below.
- `AGENTS.md` -- cross-agent guidance (placeholder; v1 targets Claude Code only).
- `LICENSE` -- MIT, matching the repo.

Validation runs via `just build-agents` from the repo root; it is folded into `just build` and the pre-commit hook.

## Tools policy

Bundled tools under `tools/` exist so skills can shell out to a tested implementation instead of restating algorithms inline (an inline 100-line script in a reference drifts; a CLI does not).

- **Permitted languages:** Python 3 (stdlib only) and POSIX shell. The plugin validator only resolves `tools/<file>.{py,sh}` links and only those extensions are recognised.
- **No JavaScript / TypeScript / Node.** Node's package ecosystem is the supply-chain attack surface this plugin will not adopt. A skill that shells out to a script the user installed via `claude plugin install` must not be able to drag a transitive `npm` dependency tree onto their machine. Python stdlib is enough for the queries skills need; if it ever stops being enough, the bar to add a language is "stdlib of a runtime users already have, no third-party packages."
- **No third-party Python packages.** Same reason: the tool must run on a stock interpreter. If you reach for `requests` or `jsonschema`, refactor.
- **Single-file by default.** A tool is one file; if it grows submodules, that's a signal to question whether the logic belongs in a skill / reference instead.

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
