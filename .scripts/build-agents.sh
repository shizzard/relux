#!/usr/bin/env bash
# Validate the agents/ plugin:
#   1. claude plugin validate -- manifest schema, SKILL.md frontmatter
#      YAML, plugin.json JSON syntax. This is the same validator
#      `claude plugin install` runs at install time, so passing here
#      means the plugin is installable.
#   1b. claude plugin validate the marketplace manifest at the repo root
#       (.claude-plugin/marketplace.json) so the plugin stays installable
#       via `claude plugin install relux@relux`.
#   2. Skill-name convention: each `skills/<dir>/SKILL.md` frontmatter
#      must declare `name: relux:<dir>`. Our project contract; not
#      enforced by Claude Code.
#   3. `references/<file>.md` links in any .md under agents/ resolve
#      to an existing file.
#   4. `relux:<name>` mentions in any .md under agents/ resolve to an
#      existing skill directory.
#   5. `tools/<file>` links in any .md under agents/ resolve to an
#      existing file under agents/tools/.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PLUGIN_DIR="agents"

err() {
    echo "ERROR: $*" >&2
}

if ! command -v claude >/dev/null 2>&1; then
    err "claude is required to validate $PLUGIN_DIR/ but was not found on PATH"
    exit 1
fi

# 1. Schema validation via the official Claude Code validator.
if ! claude plugin validate "$PLUGIN_DIR"; then
    err "claude plugin validate failed for $PLUGIN_DIR"
    exit 1
fi

# 1b. Validate the marketplace manifest (.claude-plugin/marketplace.json) so
#     `claude plugin install relux@relux` stays installable from this repo.
if ! claude plugin validate .; then
    err "claude plugin validate failed for the marketplace manifest (.claude-plugin/marketplace.json)"
    exit 1
fi

status=0

# Collect existing skill directory names.
shopt -s nullglob
skill_names=()
for d in "$PLUGIN_DIR"/skills/*/; do
    skill_names+=("$(basename "$d")")
done

is_known_skill() {
    local needle="$1"
    if [[ ${#skill_names[@]} -eq 0 ]]; then
        return 1
    fi
    local s
    for s in "${skill_names[@]}"; do
        [[ "$s" == "$needle" ]] && return 0
    done
    return 1
}

# 2. Skill-name convention check.
for d in "$PLUGIN_DIR"/skills/*/; do
    name="$(basename "$d")"
    file="${d}SKILL.md"
    fm_name="$(awk '/^---$/{c++; if(c==2) exit; next} c==1 && /^name:/{sub(/^name: */,""); print; exit}' "$file")"
    expected="relux:$name"
    if [[ "$fm_name" != "$expected" ]]; then
        err "$file frontmatter 'name' is '$fm_name' but expected '$expected'"
        status=1
    fi
done

# 3., 4., and 5. Cross-reference resolution across every .md under agents/.
while IFS= read -r f; do
    while IFS= read -r ref; do
        [[ -z "$ref" ]] && continue
        if [[ ! -f "$PLUGIN_DIR/$ref" ]]; then
            err "$f links to '$ref' but $PLUGIN_DIR/$ref does not exist"
            status=1
        fi
    done < <(grep -oE 'references/[a-z0-9._-]+\.md' "$f" 2>/dev/null | sort -u)

    while IFS= read -r mention; do
        [[ -z "$mention" ]] && continue
        target="${mention#relux:}"
        if ! is_known_skill "$target"; then
            err "$f mentions 'relux:$target' but no such skill directory exists"
            status=1
        fi
    done < <(grep -oE 'relux:[a-z][a-z0-9-]*' "$f" 2>/dev/null | sort -u)

    while IFS= read -r ref; do
        [[ -z "$ref" ]] && continue
        if [[ ! -f "$PLUGIN_DIR/$ref" ]]; then
            err "$f links to '$ref' but $PLUGIN_DIR/$ref does not exist"
            status=1
        fi
    done < <(grep -oE 'tools/[a-zA-Z0-9._/-]+\.(py|sh)' "$f" 2>/dev/null | sort -u)
done < <(find "$PLUGIN_DIR" -name '*.md' -type f)

if [[ $status -eq 0 ]]; then
    echo "plugin validation OK (${#skill_names[@]} skills)"
fi
exit $status
