#!/usr/bin/env bash
# Prove that check-cst-conformance.sh actually fires.
#
# Every case feeds canned cargo output through --from-file, so nothing here
# compiles or runs the real suite. A guard that cannot be made to fire on
# demand is not yet a guard; these are the ways this one could silently stop
# working.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
repo_root=$PWD

script=.scripts/check-cst-conformance.sh
tmp=$(mktemp -d)
# subdir is created later, inside the repo (see the --from-file cwd-resolution
# case below): the script under test calls `git rev-parse --show-toplevel`
# itself, so its caller's cwd has to be inside this working tree, unlike every
# other fixture here which lives under $tmp.
cleanup() {
    rm -rf "$tmp"
    if [[ -n "${subdir:-}" ]]; then
        rm -rf "$subdir"
    fi
    return 0
}
trap cleanup EXIT

failures=0

# Assert the script exits with the expected status, and that its output
# mentions $3 when given. Requires at least 4 args (label, want, needle, and
# a command) rather than treating the needle as optional -- a caller that
# dropped it would otherwise silently lose the first word of its command to
# an unconditional `shift 3`.
expect() {
    if [[ $# -lt 4 ]]; then
        echo "BUG: expect() needs label, want, needle, and a command (got $#)" >&2
        exit 70
    fi
    local label="$1" want="$2" needle="$3"
    shift 3
    local got=0 output
    output=$("$@" 2>&1) || got=$?
    if [[ "$got" != "$want" ]]; then
        echo "FAIL: $label -- expected exit $want, got $got"
        echo "$output" | sed 's/^/    /'
        failures=$((failures + 1))
        return
    fi
    if [[ -n "$needle" && "$output" != *"$needle"* ]]; then
        echo "FAIL: $label -- output did not mention '$needle'"
        echo "$output" | sed 's/^/    /'
        failures=$((failures + 1))
        return
    fi
    echo "ok: $label"
}

# Assert a predicate command succeeds. For assertions that are not "run the
# script and check its exit code / output" -- e.g. a file's sortedness, a
# checksum comparison, a line count.
check() {
    local label="$1"
    shift
    if "$@"; then
        echo "ok: $label"
    else
        echo "FAIL: $label"
        failures=$((failures + 1))
    fi
}

# --- predicates used by `check` above ---------------------------------------

sorted()        { LC_ALL=C sort -c "$1" >/dev/null 2>&1; }
line_count_is() { [[ $(wc -l < "$1") -eq "$2" ]]; }
seeded_with()   { [[ -f "$1" ]] && line_count_is "$1" "$2"; }
empty_file()    { [[ ! -s "$1" ]]; }
has_token()     { grep -qF -- "$2" "$1"; }

# --- fixtures --------------------------------------------------------------

cat > "$tmp/two-failures.txt" <<'EOF'
     Running unittests src/lib.rs (target/debug/deps/relux_parser-1111111111111111)
test stmt::tests::send_statement ... FAILED
test stmt::tests::other ... ok
     Running tests/ir_stmt.rs (target/debug/deps/ir_stmt-2222222222222222)
test lower_send_statement ... FAILED
test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
EOF

# Same content, with the ANSI escapes CARGO_TERM_COLOR=always adds to the
# Running headers. libtest's own lines are never coloured when piped.
printf '\033[1m\033[92m     Running\033[0m unittests src/lib.rs (target/debug/deps/relux_parser-1111111111111111)\ntest stmt::tests::send_statement ... FAILED\n\033[1m\033[92m     Running\033[0m tests/ir_stmt.rs (target/debug/deps/ir_stmt-2222222222222222)\ntest lower_send_statement ... FAILED\ntest result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out\n' \
    > "$tmp/coloured.txt"

cat > "$tmp/no-header.txt" <<'EOF'
test lower_send_statement ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
EOF

cat > "$tmp/colliding-stems.txt" <<'EOF'
     Running tests/corpus.rs (target/debug/deps/corpus-3333333333333333)
test a ... FAILED
     Running tests/corpus.rs (target/debug/deps/corpus-4444444444444444)
test b ... FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
EOF

# relux-cli has both a lib target and a bin target named `relux`, so cargo
# builds two `relux-<hash>` binaries. The key must tell them apart (a `_bin`
# suffix on the bin target) rather than colliding through assert_unique_binaries.
cat > "$tmp/lib-bin-pair.txt" <<'EOF'
     Running unittests src/lib.rs (target/debug/deps/relux-5555555555555555)
test lib_only_test ... FAILED
     Running unittests bin/relux.rs (target/debug/deps/relux-6666666666666666)
test bin_only_test ... FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
EOF

# A failing test's captured stdout is replayed after a `failures:` line, and
# this repository's own tests assert on test-runner-shaped output, so a
# `test x ... FAILED` string can appear verbatim in that replay without being
# a real result. Only stmt::tests::send_statement is a real failure here.
cat > "$tmp/replayed-output.txt" <<'EOF'
     Running unittests src/lib.rs (target/debug/deps/relux_parser-1111111111111111)
test stmt::tests::send_statement ... FAILED
test stmt::tests::other ... ok

failures:

---- stmt::tests::send_statement stdout ----
thread 'stmt::tests::send_statement' panicked at src/stmt.rs:10:
assertion failed: `(left == right)`
note: captured subprocess output included the literal line below
test x ... FAILED

failures:
    stmt::tests::send_statement

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
EOF

cat > "$tmp/all-pass.txt" <<'EOF'
     Running unittests src/lib.rs (target/debug/deps/relux_parser-1111111111111111)
test stmt::tests::other ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
EOF

# Doctest names are `<path> - <item> (line N)`: they contain spaces, so
# field-splitting on the FAILED line (rather than capturing everything
# between `test ` and ` ... FAILED`) would truncate the name to its first
# word and merge two failing doctests together after sort -u. The `Doc-tests`
# header also reports the Cargo.toml package name verbatim (hyphenated),
# which must be normalized like every other key (underscored).
cat > "$tmp/doctest-failure.txt" <<'EOF'
   Doc-tests relux-core
test src/lib.rs - foo::bar (line 12) ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
EOF

: > "$tmp/empty.txt"

exact() {
    cat > "$tmp/list.txt" <<'EOF'
ir_stmt lower_send_statement
relux_parser stmt::tests::send_statement
EOF
}

# A fake `cargo` on PATH. The no-args path is the only one that runs cargo,
# and every other case here bypasses it with --from-file. This shim pins the
# three things that path must get right: --no-fail-fast, whose absence
# silently truncates the failure set to the first failing binary; the
# CARGO_TERM_COLOR=never override that defeats the global `always` set in
# ci.yml; and the feature name itself.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/cargo" <<'SHIM'
#!/usr/bin/env bash
{
    echo "ARGV: $*"
    echo "ENV: CARGO_TERM_COLOR=${CARGO_TERM_COLOR:-unset}"
} >> "$RECORD"
cat <<'OUT'
     Running unittests src/lib.rs (target/debug/deps/relux_parser-1111111111111111)
test stmt::tests::send_statement ... FAILED
     Running tests/ir_stmt.rs (target/debug/deps/ir_stmt-2222222222222222)
test lower_send_statement ... FAILED
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
OUT
SHIM
chmod +x "$tmp/bin/cargo"

# --- cases -----------------------------------------------------------------

exact
expect "exact match passes" 0 "" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt"

exact
printf 'ir_stmt a_test_that_now_passes\n' >> "$tmp/list.txt"
LC_ALL=C sort -o "$tmp/list.txt" "$tmp/list.txt"
expect "listed test that now passes is an error" 1 "now PASS" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt"

exact
grep -v lower_send_statement "$tmp/list.txt" > "$tmp/l2" && mv "$tmp/l2" "$tmp/list.txt"
expect "unlisted failure is an error" 1 "is a regression" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt"

# A list that is not C-sorted makes `comm` lie about which side an entry is
# on; the script must refuse to compare against one instead of guessing.
cat > "$tmp/list.txt" <<'EOF'
relux_parser stmt::tests::send_statement
ir_stmt lower_send_statement
EOF
expect "unsorted list is rejected" 1 "not sorted" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt"

# A duplicated entry survives `sort` but not `sort -u`, so it trips the same
# check for the same reason: the file on disk no longer equals its own
# canonical form.
cat > "$tmp/list.txt" <<'EOF'
ir_stmt lower_send_statement
ir_stmt lower_send_statement
relux_parser stmt::tests::send_statement
EOF
expect "duplicated list entry is rejected" 1 "not sorted" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt"

exact
grep -v lower_send_statement "$tmp/list.txt" > "$tmp/l2" && mv "$tmp/l2" "$tmp/list.txt"
before_checksum=$(cksum "$tmp/list.txt")
expect "--bless refuses to add without --allow-additions" 1 "--allow-additions" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt" --bless
checksum_unchanged() { [[ "$(cksum "$tmp/list.txt")" == "$before_checksum" ]]; }
check "--bless refusal left the list unchanged" checksum_unchanged

expect "--bless --allow-additions rewrites the list" 0 "" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt" \
        --bless --allow-additions
check "--bless --allow-additions wrote both failures" line_count_is "$tmp/list.txt" 2
check "--bless wrote a sorted list" sorted "$tmp/list.txt"

# --bless must be able to seed a list that does not exist yet -- this is how
# known_failing.txt gets created in the first place.
rm -f "$tmp/absent.txt"
expect "--bless can seed a missing list" 0 "" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/absent.txt" \
        --bless --allow-additions
check "--bless seeded the missing list" seeded_with "$tmp/absent.txt" 2

# --bless must also be able to repair a list that already has the right
# content but the wrong order -- the sortedness check that would otherwise
# reject it is exactly what --bless exists to fix.
cat > "$tmp/list.txt" <<'EOF'
relux_parser stmt::tests::send_statement
ir_stmt lower_send_statement
EOF
expect "--bless can repair an unsorted list" 0 "" \
    "$script" --from-file "$tmp/two-failures.txt" --list "$tmp/list.txt" --bless
check "--bless repaired the unsorted list" sorted "$tmp/list.txt"

exact
expect "ANSI-coloured headers still parse" 0 "" \
    "$script" --from-file "$tmp/coloured.txt" --list "$tmp/list.txt"

exact
expect "empty cargo output is not 'no failures'" 1 "did not run" \
    "$script" --from-file "$tmp/empty.txt" --list "$tmp/list.txt"

exact
expect "a failure with no binary header is an error" 1 "no preceding binary" \
    "$script" --from-file "$tmp/no-header.txt" --list "$tmp/list.txt"

exact
expect "two binaries sharing a stem is an error" 1 "produce the same list key" \
    "$script" --from-file "$tmp/colliding-stems.txt" --list "$tmp/list.txt"

rm -f "$tmp/list.txt"
expect "a lib/bin target pair does not collide" 0 "" \
    "$script" --from-file "$tmp/lib-bin-pair.txt" --list "$tmp/list.txt" \
        --bless --allow-additions
lib_bin_keys_distinct() {
    [[ $(wc -l < "$tmp/list.txt") -eq 2 ]] \
        && grep -qx 'relux lib_only_test' "$tmp/list.txt" \
        && grep -qx 'relux_bin bin_only_test' "$tmp/list.txt"
}
check "lib and bin targets got distinct keys" lib_bin_keys_distinct

cat > "$tmp/list.txt" <<'EOF'
relux_parser stmt::tests::send_statement
EOF
expect "a FAILED line replayed inside a failures: block is not collected" 0 "" \
    "$script" --from-file "$tmp/replayed-output.txt" --list "$tmp/list.txt"

rm -f "$tmp/list.txt"
expect "a doctest failure keeps its full name and an underscored key" 0 "" \
    "$script" --from-file "$tmp/doctest-failure.txt" --list "$tmp/list.txt" \
        --bless --allow-additions
doctest_entry_is_intact() {
    line_count_is "$tmp/list.txt" 1 \
        && grep -qxF 'relux_core_doctests src/lib.rs - foo::bar (line 12)' "$tmp/list.txt"
}
check "doctest entry has the full name and normalized key" doctest_entry_is_intact

# A fully green suite against a non-empty list must report every entry as
# now-passing, not silently succeed.
exact
expect "all-green output against a non-empty list is an error" 1 "now PASS" \
    "$script" --from-file "$tmp/all-pass.txt" --list "$tmp/list.txt"

# An empty $observed fed through `printf '%s\n'` still emits one blank line,
# which comm can read back as a phantom failing test. Guard against that
# regressing: an all-green run must never report an unlisted regression.
exact
no_phantom_regression() {
    local out
    out=$("$script" --from-file "$tmp/all-pass.txt" --list "$tmp/list.txt" 2>&1) || true
    [[ "$out" != *"is a regression"* ]]
}
check "all-green output reports no phantom regression" no_phantom_regression

# ...and blessing that same output must be able to empty the list.
expect "--bless can empty the list" 0 "" \
    "$script" --from-file "$tmp/all-pass.txt" --list "$tmp/list.txt" --bless
check "--bless emptied the list" empty_file "$tmp/list.txt"

exact
: > "$tmp/rec.txt"
# CARGO_TERM_COLOR=always in the caller's environment is what CI does.
expect "no-args path runs cargo with the right flags" 0 "" \
    env RECORD="$tmp/rec.txt" PATH="$tmp/bin:$PATH" CARGO_TERM_COLOR=always \
        "$script" --list "$tmp/list.txt"

for token in "--no-fail-fast" "--features relux-parser/cst-frontend" "CARGO_TERM_COLOR=never"; do
    check "cargo invoked with '$token'" has_token "$tmp/rec.txt" "$token"
done

# --- argument handling -------------------------------------------------------

expect "--help exits 0 and shows usage" 0 "Usage:" "$script" --help

expect "--from-file with no argument is a usage error, not a crash" 2 "" \
    "$script" --from-file

expect "a nonexistent --from-file path is diagnosed, not misread as no results" 1 \
    "does not exist" \
    "$script" --from-file "$tmp/does-not-exist.txt" --list "$tmp/list.txt"

# The script under test calls `git rev-parse --show-toplevel` itself, so this
# has to run from somewhere inside the repo -- $tmp is not a git working tree.
exact
subdir=$(mktemp -d "crates/relux-selftest-XXXXXX")
cp "$tmp/two-failures.txt" "$subdir/"
relative_from_file_resolves_to_caller_cwd() {
    (cd "$subdir" && "$repo_root/$script" --from-file two-failures.txt --list "$tmp/list.txt")
}
check "a relative --from-file resolves against the caller's cwd" \
    relative_from_file_resolves_to_caller_cwd
rm -rf "$subdir"
unset subdir

# --- verdict ---------------------------------------------------------------

if (( failures )); then
    echo
    echo "$failures self-test case(s) failed."
    exit 1
fi
echo
echo "all conformance harness self-tests passed"
