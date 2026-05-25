# AGENTS.md

Cross-agent guidance for the Relux plugin lives here.

v1 targets Claude Code only; `.claude-plugin/plugin.json` is the active distribution channel. The content under `skills/` and `references/` is portable Markdown -- no Claude-specific syntax beyond tool-name references (Read, Edit, Bash) and cross-skill handoff sentences.

Future per-agent manifests (Cursor, OpenCode, Gemini CLI, Codex) sit alongside `.claude-plugin/` and reuse the portable content unchanged.
