#!/usr/bin/env bash
# Validate the agents/ plugin:
#   1. plugin.json parses and has required fields.
#   2. Every skill directory has a SKILL.md with valid frontmatter
#      (name == "relux:<dir>"; description is non-empty).
#   3. Every references/<file>.md link mentioned in any .md under
#      agents/ resolves to an existing file.
#   4. Every relux:<name> mention -- in any section of any .md under
#      agents/, not just Cross-skill handoffs -- resolves to an
#      existing skill directory. The token is the contract; there
#      are no "soft" references that escape validation.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PLUGIN_DIR="agents"
MANIFEST="$PLUGIN_DIR/.claude-plugin/plugin.json"

err() {
    echo "ERROR: $*" >&2
}

if ! command -v jq >/dev/null 2>&1; then
    err "jq is required to validate $MANIFEST but was not found on PATH"
    exit 1
fi

status=0

# 1. Manifest exists, parses, and has required fields.
if [[ ! -f "$MANIFEST" ]]; then
    err "missing plugin manifest at $MANIFEST"
    exit 1
fi

if ! jq empty "$MANIFEST" >/dev/null 2>&1; then
    err "$MANIFEST is not valid JSON"
    exit 1
fi

for field in name description author version; do
    val="$(jq -r --arg f "$field" '.[$f] // ""' "$MANIFEST")"
    if [[ -z "$val" ]]; then
        err "$MANIFEST is missing required field '$field'"
        status=1
    fi
done

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

# 2. SKILL.md frontmatter check.
for d in "$PLUGIN_DIR"/skills/*/; do
    name="$(basename "$d")"
    file="${d}SKILL.md"
    if [[ ! -f "$file" ]]; then
        err "skill directory '$d' has no SKILL.md"
        status=1
        continue
    fi
    fm="$(awk '/^---$/{c++; if(c==2) exit; next} c==1' "$file")"
    fm_name="$(printf '%s\n' "$fm" | awk -F': *' '/^name:/{print $2; exit}')"
    fm_desc="$(printf '%s\n' "$fm" | awk -F': *' '/^description:/{print $2; exit}')"
    expected="relux:$name"
    if [[ "$fm_name" != "$expected" ]]; then
        err "$file frontmatter 'name' is '$fm_name' but expected '$expected'"
        status=1
    fi
    if [[ -z "$fm_desc" ]]; then
        err "$file frontmatter is missing 'description'"
        status=1
    fi
done

# 3. and 4. Scan every .md file under agents/ for link / mention resolution.
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
done < <(find "$PLUGIN_DIR" -name '*.md' -type f)

if [[ $status -eq 0 ]]; then
    echo "plugin validation OK (${#skill_names[@]} skills)"
fi
exit $status
