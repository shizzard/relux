#!/usr/bin/env bash
# Check the CST front end against the known-failing list. Run with --help
# for usage.
set -euo pipefail

usage() {
    cat <<'USAGE'
Check the CST front end against the known-failing list.

Runs the workspace suite with relux-parser/cst-frontend enabled and compares
the set of failing tests against known_failing.txt. Fails in BOTH directions:
a listed test that now passes must be removed, and an unlisted test that
fails is a regression. The second direction is what makes the list a ratchet
rather than a wish.

Usage:
  check-cst-conformance.sh
      Run the workspace suite and compare against the list.
  check-cst-conformance.sh --from-file PATH
      Compare canned cargo output instead of running the suite.
  check-cst-conformance.sh --list PATH
      Use a known-failing list other than the default.
  check-cst-conformance.sh --bless
      Rewrite the list to match what was observed. Refuses to add entries.
  check-cst-conformance.sh --bless --allow-additions
      As above, but also allowed to add entries (e.g. to seed the list).
  check-cst-conformance.sh --help
      Show this message.
USAGE
}

orig_pwd=$PWD
list=crates/relux-parser/tests/known_failing.txt
list_explicit=0
from_file=""
bless=0
allow_additions=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            usage
            exit 0
            ;;
        --from-file)
            [[ $# -ge 2 ]] || { echo "ERROR: --from-file requires a path argument" >&2; exit 2; }
            from_file="$2"
            shift 2
            ;;
        --list)
            [[ $# -ge 2 ]] || { echo "ERROR: --list requires a path argument" >&2; exit 2; }
            list="$2"
            list_explicit=1
            shift 2
            ;;
        --bless)
            bless=1
            shift
            ;;
        --allow-additions)
            allow_additions=1
            shift
            ;;
        *)
            echo "ERROR: unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

# --from-file and an explicit --list are caller-relative. Resolve them
# against the caller's cwd now, before cd'ing to the repo root below, or a
# relative path would silently resolve against the repo root instead of
# where the caller actually is. The default --list is deliberately
# repo-root-relative and is left alone.
case "$from_file" in
    ""|/*) ;;
    *) from_file="$orig_pwd/$from_file" ;;
esac
if (( list_explicit )); then
    case "$list" in
        /*) ;;
        *) list="$orig_pwd/$list" ;;
    esac
fi

cd "$(git rev-parse --show-toplevel)"

if [[ -n "$from_file" && ! -f "$from_file" ]]; then
    echo "ERROR: --from-file path does not exist: $from_file" >&2
    exit 1
fi

# CI sets CARGO_TERM_COLOR=always, which colours cargo's own `Running` headers.
# libtest's `test ... FAILED` lines are never coloured when piped, so a naive
# grep passes locally and matches nothing in CI.
strip_ansi() { sed -e 's/\x1b\[[0-9;]*m//g'; }

# Shared by collect_failures and assert_unique_binaries: turns one cargo
# `Running`/`Doc-tests` header line into the failure-list key (and, for a
# `Running` line, the build hash) for that binary.
#
# The key is the dep-stem -- `relux_parser` for a lib target, `ir_stmt` for
# tests/ir_stmt.rs -- plus a `_bin` suffix when the binary under test is a
# `bin/` target rather than `src/lib.rs`. That makes the key unique by
# construction for the case that actually occurs in this workspace: a crate
# with both a lib and a bin target of the same name (relux-cli has both,
# built as two `relux-<hash>` binaries that would otherwise collide).
# assert_unique_binaries below catches what construction does not cover --
# e.g. two integration-test files that happen to share a basename.
#
# Doc-tests headers report the crate's Cargo.toml package name, which uses
# hyphens (`relux-core`); every other key is the underscored build-target
# stem, so the package name is normalized the same way for consistency.
awk_header_fn='
function parse_header(line, out,    desc, hpath, base, hash, stem, pkg) {
    if (line ~ /^[[:space:]]*Running[[:space:]]+.*\(target\/[^)]*\)$/) {
        desc = line
        sub(/^[[:space:]]*Running[[:space:]]+/, "", desc)
        hpath = desc
        sub(/^.*\(/, "", hpath)
        sub(/\)[[:space:]]*$/, "", hpath)
        sub(/[[:space:]]*\([^)]*\)[[:space:]]*$/, "", desc)
        base = hpath
        sub(/^.*\//, "", base)
        hash = base
        sub(/^.*-/, "", hash)
        stem = base
        sub(/-[0-9a-f]+$/, "", stem)
        out["key"]  = (desc ~ /^unittests[[:space:]]+bin\//) ? stem "_bin" : stem
        out["hash"] = hash
        return 1
    }
    if (line ~ /^[[:space:]]*Doc-tests[[:space:]]+/) {
        pkg = line
        sub(/^[[:space:]]*Doc-tests[[:space:]]+/, "", pkg)
        gsub(/-/, "_", pkg)
        out["key"]  = pkg "_doctests"
        out["hash"] = ""
        return 1
    }
    return 0
}
'

# `bin` tracks the current binary's key. It resets on every header, and also
# on a `failures:` line -- libtest re-prints failing tests' captured stdout
# after that line, and this repository's own tests assert on test-runner-
# shaped output, so a `test ... FAILED` string can appear verbatim inside
# that replay without being a real result. Suppressing collection from
# `failures:` until the next header avoids treating replayed output as a
# result; the real result lines were already collected from the `running N
# tests` section above.
collect_failures() {
    strip_ansi < "$1" | awk "$awk_header_fn"'
        BEGIN { bin = ""; skip = 0 }
        {
            if (parse_header($0, hdr)) {
                bin = hdr["key"]
                skip = 0
                next
            }
            if ($0 ~ /^failures:$/) {
                skip = 1
                next
            }
            if (skip) next
            if ($0 ~ /^test .* \.\.\. FAILED$/) {
                name = $0
                sub(/^test /, "", name)
                sub(/ \.\.\. FAILED$/, "", name)
                if (bin == "") {
                    print "ERROR: no preceding binary header for failing test: " name > "/dev/stderr"
                    exit 1
                }
                print bin, name
            }
        }
    ' | LC_ALL=C sort -u
}

# Two binaries whose keys collide would silently merge into one entry, and
# the merge is invisible in the failure list.
assert_unique_binaries() {
    local dupes
    dupes=$(strip_ansi < "$1" | awk "$awk_header_fn"'
        { if (parse_header($0, hdr) && hdr["hash"] != "") print hdr["key"], hdr["hash"] }
    ' | LC_ALL=C sort -u | awk '{print $1}' | uniq -d)

    if [[ -n "$dupes" ]]; then
        echo "ERROR: two test binaries produce the same list key:" >&2
        echo "$dupes" | sed 's/^/    /' >&2
        echo "The key already encodes stem + target kind (lib vs bin), so this" >&2
        echo "means either two integration-test files share a basename, or the" >&2
        echo "key derivation in this script needs another case. Rename the" >&2
        echo "colliding file, or fix the key derivation." >&2
        exit 1
    fi
}

# --- obtain cargo output ---------------------------------------------------

if [[ -n "$from_file" ]]; then
    out="$from_file"
else
    out=$(mktemp)
    trap 'rm -f "$out"' EXIT
    # --no-fail-fast is load-bearing. Without it cargo stops after the first
    # failing binary and the observed set is silently short, which looks
    # exactly like a passing ratchet. A failing suite is expected here, so the
    # exit status is ignored and the sanity check below is what catches a
    # build that never ran.
    CARGO_TERM_COLOR=never cargo test --workspace --no-fail-fast \
        --features relux-parser/cst-frontend > "$out" 2>&1 || true
fi

if ! grep -q '^test result:' "$out"; then
    echo "ERROR: no test results in cargo output -- the suite did not run." >&2
    echo "Last 20 lines:" >&2
    tail -20 "$out" | sed 's/^/    /' >&2
    exit 1
fi

assert_unique_binaries "$out"
observed=$(collect_failures "$out")

# --- compare ---------------------------------------------------------------

if [[ ! -f "$list" ]]; then
    if (( bless )); then
        # Seeding: an absent list is an empty one, so seeding the list for
        # the first time must not be blocked by its own absence.
        : > "$list"
    else
        echo "ERROR: $list does not exist. Seed it with --bless --allow-additions." >&2
        exit 1
    fi
fi

# An unsorted or duplicated list makes `comm` lie. `--bless` rewrites it sorted,
# so it is the repair path and must not be gated on the condition it repairs.
if (( ! bless )) && ! diff -q <(LC_ALL=C sort -u "$list") "$list" >/dev/null; then
    echo "ERROR: $list is not sorted and deduplicated. Run --bless." >&2
    exit 1
fi

# comm collates under the ambient locale. On glibc with e.g. en_US.UTF-8 that
# disagrees with the C-locale sort used to validate (and to write) the list
# above -- a mismatch that does not reproduce on macOS, where LC_COLLATE is
# already C, but does reproduce on the ubuntu runners CI uses.
#
# comm also hard-requires sorted input and exits nonzero the instant it isn't
# (fatal under set -e). The sortedness check above is skipped in bless mode
# on purpose -- bless is the repair path for exactly that defect -- so feed
# comm a freshly C-sorted view of the list rather than the file as it sits on
# disk; outside bless mode the list is already sorted and this is a no-op.
fixed=$(LC_ALL=C comm -23 <(LC_ALL=C sort -u "$list") <(printf '%s\n' "$observed"))
regressed=$(LC_ALL=C comm -13 <(LC_ALL=C sort -u "$list") <(printf '%s\n' "$observed"))

if (( bless )); then
    if [[ -n "$regressed" && $allow_additions -eq 0 ]]; then
        echo "ERROR: --bless would ADD $(printf '%s\n' "$regressed" | wc -l) entries:" >&2
        printf '%s\n' "$regressed" | sed 's/^/    /' >&2
        echo >&2
        echo "Once seeded, the list only ever shrinks, so an addition means a" >&2
        echo "grammar task broke something it does not own." >&2
        echo "Pass --allow-additions only when seeding the list." >&2
        exit 1
    fi
    printf '%s' "$observed" > "$list"
    [[ -n "$observed" ]] && printf '\n' >> "$list"
    echo "blessed $list ($(wc -l < "$list") entries)"
    exit 0
fi

status=0

if [[ -n "$fixed" ]]; then
    echo "These tests are listed as known-failing but now PASS:"
    printf '%s\n' "$fixed" | sed 's/^/    /'
    echo
    echo "Remove them by running: .scripts/check-cst-conformance.sh --bless"
    echo "(no --allow-additions needed -- that flag only guards against additions)"
    echo
    status=1
fi

if [[ -n "$regressed" ]]; then
    echo "These tests FAIL but are not listed. This is a regression:"
    printf '%s\n' "$regressed" | sed 's/^/    /'
    echo
    status=1
fi

if (( status == 0 )); then
    echo "CST conformance: $(wc -l < "$list") known-failing, no regressions"
fi

exit "$status"
