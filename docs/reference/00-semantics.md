# Relux Semantic Model

## Modules

- Every `.relux` file is a module
- A module can contain any combination of: imports, functions, effects, tests
- There is no distinction between "library" and "test" modules
- Module path is its filesystem path relative to the project root (e.g. `lib/matchers` resolves to `lib/matchers.relux`)
- The project root is defined by the location of `Relux.toml`

## Imports

- Imports resolve from the project root, never relative to the importing file
- Selective imports bring specific names into scope: `import lib/m { foo, bar, StartDb }`
- Wildcard imports bring all names into scope: `import lib/m`
- `as` aliases rename an imported name locally: `foo as f`, `StartDb as Db`
- Aliases must preserve the casing kind: lowercase names get lowercase aliases, CamelCase names get CamelCase aliases
- Each module is loaded once regardless of how many files import it
- Circular imports are a parse error

## Variables

- All variable values are strings, no other types exist
- Uninitialized variables (`let x`) default to empty string `""`
- Variables are scoped to their enclosing block (`test`, `shell`, `fn`, `effect`)
- Inner blocks can shadow outer variables with a new `let` declaration
- Binding uses `:=`: declaration with a value (`let x := expr`), reassignment (`x := expr`), and overlay entries (`{ KEY := expr }`) all use it; bare `=` is no longer a binding operator (it serves as the literal-match arm inside a multimatch block)
- Reassignment (`x := expr`) mutates an existing variable from an outer scope
- Environment variables from the host process are available as pre-set variables in all scopes (read-only — `let` creates a shadow, not a modification of the process environment)
- Hierarchical `.env` files, when present, layer over the host process environment and take precedence over it; their values feed interpolation, marker evaluation, and the shell under test
- Regex capture groups (`$1`, `$2`, ...) are set after a `<?` match or a `?` [pure match](08-pure-matching.md) and remain in scope until overwritten by the next regex match. Inside a `pure fn` body a `?` pure match binds `$1..$n` into the function's own per-call frame, discarded when the function returns (they do not leak to the caller, just as a called `fn`'s captures do not leak out)

## Functions

- Function and shell names must start with a lowercase letter or underscore (`snake_case`) — this is enforced at the syntactic level
- Functions are reusable sequences of statements
- A function executes in the caller's shell context — it has no shell of its own
- Functions can only be called inside `shell` blocks (since shell operators require an active shell)
- The return value is the last expression's value in the body
- If the caller doesn't capture the return value, it is discarded
- Side effects persist in the caller's shell: a function that sets `~30s` or `!? error` changes the shell's timeout/fail-pattern for subsequent statements
- Functions can call other functions
- Functions can use imports from their own module

## Pure Functions

- Declared with `pure fn` instead of `fn`
- Cannot contain shell operators (`>`, `=>`, `<?`, `<=`, `!?`, `!=`, timeouts)
- Cannot call impure built-in functions (e.g., `match_prompt()`, `ctrl_c()`)
- Cannot call regular `fn` functions — only other pure functions and pure built-in functions
- Can only contain: `let` declarations, variable reassignment, expressions, and [pure-match](08-pure-matching.md) statements (`<expr> = <pattern>` / `<expr> ? <pattern>`)
- A `?` pure match inside the body binds `$1..$n` into the call's own capture frame, so a `pure fn` can extract a value and return it; a non-matching pure match fails the test through the runtime site that called the function (a marker condition is the exception — a pure-eval failure there is falsy, not a test failure)
- Can be called from condition markers, overlay expressions, and regular shell blocks
- "Pure" means shell-independent, not side-effect-free — pure BIFs like `sleep()` and `log()` are allowed

## Shells

- A shell is a spawned PTY process (default: `/bin/sh`)
- `stdout` and `stderr` are merged into a single output stream
- Send operators (`>`, `=>`) write to the shell's `stdin`
- Match operators (`<?`, `<=`) assert against the shell's accumulated output
- Match operations block until a match is found or the timeout expires
- A timeout expiry is a test failure
- Any match operator can include an inline timeout override (`<~dur` or `<@dur`):
  - Applies only to that single operation (one-shot)
  - Does not affect the shell's scoped timeout
  - Duration uses compact humantime format (no spaces): `2s`, `500ms`, `1m30s`
- Timeouts come in two kinds:
  - **Tolerance** (`~`) — scaled by `--timeout-multiplier`. Used for operations that may be slower under load
  - **Assertion** (`@`) — never scaled. Used to assert the system responds within a hard deadline
- Each shell has one active fail pattern slot — if shell output matches the fail pattern, the test fails immediately
  - Fail patterns are checked inline during match operations (under the same lock as consume) and at statement boundaries
  - Setting a fail pattern immediately rescans the buffer for the pattern
  - An empty fail pattern operator (`!?` or `!=` with no payload) clears the active fail pattern
- A match operator with no payload (`<?` or `<=` with nothing after it) resets the output buffer cursor, consuming all current output
- Each shell has one active timeout value, initially set to a framework default
- Multiple `shell <name>` blocks with the same name in a test/effect refer to the same shell (switching the active shell, like lux's `[shell name]`)
- A multi-pattern match block (`<{ <line>+ }`) waits for several patterns at once:
  - Each inner line is `? <pattern>` (regex) or `= <pattern>` (literal); patterns are mixed freely
  - The block is atomic with respect to the cursor: every byte that arrives is offered to every still-unmatched pattern, the cursor sits still until block exit
  - A pattern transitions to matched the first time it succeeds against the slice `[block_entry, current_buffer_end]`; once matched, it no longer participates in subsequent scans
  - The block completes when every pattern has matched at least once
  - At block exit the cursor advances once, to the maximum of the per-pattern match-end offsets (overlapping matches are permitted; duplicate inner patterns are independent slots that may land on the same bytes)
  - Capture groups in inner regex patterns do not bind - `$n` is not written by a multimatch block
  - If the block timeout fires before all patterns have matched, the test fails; the failure record lists every pattern with its matched/unmatched status
  - Fail patterns remain active during the block; a fail-pattern hit aborts the block exactly as it would abort a single `<?` / `<=`
  - The inline-timeout prefix shape `<~Ns{ ... }` / `<@Ns{ ... }` carries the standard tolerance/assertion semantics, applied to the whole block

## Pure Matching

- A **pure match** asserts that a computed value satisfies a pattern; it reads nothing from a shell (contrast the `<?` / `<=` operators, which scan the output buffer)
- Two statement forms, valid in `shell` blocks, `fn` bodies, `pure fn` bodies, and **test / effect preambles** (alongside `let`, before the first `shell` block):
  - `<expr> = <pattern>` — asserts `<expr>` equals `<pattern>` exactly (byte-for-byte literal equality; a superstring or partial overlap fails). This is the same exact-equality semantics as the marker `=` operator
  - `<expr> ? <pattern>` — asserts `<expr>` matches the regex `<pattern>` (unanchored, like `<?`)
- `<pattern>` is an interpolated string (`${var}` resolved before comparison); `<expr>` is any pure expression
- A pure match is an assertion: a no-match is an immediate test failure (Relux has no error handling), the same as a `<?` that never matches. There is no negated form and no timeout — the value is already in hand, so the match either succeeds or fails at once
- A successful `?` binds the numeric capture groups `$0`, `$1`, ..., `$n` in the current shell, exactly like `<?`; they persist until the next regex match overwrites them. The `=` form binds no captures
- A `?` pattern that fails to compile (only reachable when interpolation produces malformed regex) is a runtime failure, not a no-match
- Inside a `pure fn` body, a `?` pure match binds numeric captures into the function's own per-call capture frame (each call starts empty), so a `pure fn` can match `$1` out of a value and return it. A no-match inside a `pure fn` fails the test through the runtime site that called it (`let`, overlay, shell-block call); inside a marker condition, a pure-eval failure instead makes the condition falsy rather than failing the test
- In a test or effect **preamble**, a `?` pure match binds `$n` into a single capture frame hoisted across the whole preamble: later preamble `let`s, later preamble pure matches, and a `start` overlay's expressions all read it. A non-matching preamble pure match fails the test (test-level) or fails effect instantiation (effect-level). `$n` reads the ambient frame uniformly, rendering `""` when no regex has run, so a `$n` with no preceding match is not an error
- **Shells do not inherit the preamble capture frame.** A `shell` block owns its own frame, populated only by its own `<?` matches, starting empty; `$n` inside a shell reads the shell's frame, never the preamble's. Carry a preamble capture into a shell by binding it to a `let`. Markers run before the preamble, so a marker condition sees an empty frame
- Statement-only: a pure match cannot be a `let` right-hand side, an overlay value, or a cleanup value. `x = e` asserts; binding still requires `x := e` (and `let x = e` remains an error)

## Effects

- Effect names must start with an uppercase letter (`CamelCase`) — this is enforced at the syntactic level, disambiguating effects from functions in imports
- An effect is a reusable setup procedure that produces running shells and computed values
- An effect has three explicit interface components:
  - **`expect`** — declares required environment variables the effect reads; the resolver validates these are satisfiable
  - **`expose`** — declares which shells and variables the effect makes available to callers; the `expose` keyword requires a `shell` or `var` discriminator (`expose shell db`, `expose var port`); internal shells not listed in `expose` are terminated after setup
  - **`start`** — declares dependency effects with optional env remapping via overlay
- None of these declarations are mandatory: an effect may have no `expect`, no `start`, and no `expose`
- `start Effect` runs the dependency for side effects only — its shells are not accessible
- `start Effect as Alias` runs the dependency and makes its exposed shells/variables available via dot-access (`shell Alias.shell_name`, `${Alias.var_name}`)
- Effect aliases (the name after `as`) must be CamelCase, matching effect naming conventions
- `start Effect as Alias { KEY := expr }` provides an overlay that remaps the caller's environment into the dependency's environment
  - The shorthand form `KEY` (without `:= expr`) is equivalent to `KEY := KEY`
- An overlay value may also be a qualified reference to a sibling start's exposed variable: `start Api { DB_PORT := Db.port }`. This induces an implicit dependency — `Api` is instantiated only after `Db` is ready, and reads `Db`'s exposed value at that point; independent siblings still start in parallel, only the data-dependent edge is serialized
  - The referenced start must be aliased (`start Effect as Alias`); an unaliased start cannot be referenced. The referenced effect must `expose var` the referenced variable — referencing a non-exposed or internal binding is a compile error
  - Only variables are referenceable this way: overlay values are strings, so `Alias.var` can name a sibling's `expose var`, never its `expose shell` — an exposed shell is not a string value
  - Forward references are legal: a start may reference a sibling declared later in the same start-list. A reference cycle among siblings (`A { X := B.out }` and `B { Y := A.out }` in the same start-list) is a compile error
  - The rule applies uniformly to test-level and effect-body start-lists, and the reference works nested inside interpolation too: `URL := "${Db.host}:${Db.port}"`
  - This is the preferred way to route a value an effect produces to a sibling effect that needs it. The older pattern — hoist the value to a test-level `let` and feed it into both effects' overlays — still compiles and runs unchanged
- Effects inherit the full parent environment — overlay entries override specific keys
- Effect instance identity is determined by `(effect-name, evaluated overlay restricted to expect-declared vars)`:
  - Same identity tuple = same instance (deduplicated, reused)
  - Different identity tuple = separate instances
- When a test or effect starts the same effect multiple times with the same evaluated overlay, only one instance is created
- A dependent's identity includes any value it sources from a sibling via the `Alias.var` overlay form above, exactly like any other overlay value: two dependents wired to different producers (e.g. two `Api` instances reading different `Db.port` values) get distinct instances
- Exposed shells are accessed via dot notation: `shell Alias.shell_name { ... }`
- Exposed variables are accessed via dot notation in interpolation: `${Alias.var_name}`
- A qualified reference `${Alias.var}` in a `shell` body or `cleanup` body — at effect level and at test level alike — is validated at compile time: `Alias` must be one of the enclosing effect's or test's own `start` dependency aliases, and that dependency must `expose var` the referenced variable, or it is a compile error naming the offending qualifier and variable. (Previously this went unvalidated and resolved to the empty string at runtime.)
- Exposed variables are only accessible in shell contexts (runtime); test-level and effect-level `let` bindings cannot reference them (purity violation — `let` is evaluated at resolve time, before effects are started)
- Exposed variables are read-only from the caller's perspective
- For composed effects, `expose` can re-export a dependency's shell or variable: `expose shell Dep.shell as public_name`, `expose var Dep.port as db_port`
- An effect's setup preamble may contain [pure-match](08-pure-matching.md) statements alongside its `let`s; a `?` match binds `$n` into a preamble capture frame that later setup `let`s and `start` overlays read (the effect's own `shell` blocks do not inherit it). A non-matching setup pure match fails effect instantiation, failing every test that depends on the effect
- Effects run before the test body; the dependency graph is resolved and executed in topological order
- Circular effect dependencies are a parse error
- If an effect fails (a match times out during setup), all tests depending on it are failed
- Each effect has an optional `cleanup` block that runs when the effect is torn down

## Condition Markers

- Condition markers are placed immediately before `test`, `effect`, `fn`, or `pure fn` declarations
- Condition markers evaluate **before** any shells are spawned
  - Test-level markers are checked before `execute_effects`
  - Effect-level markers are checked before the effect's shells are created
  - Function-level markers are checked during resolution; a skipped function causes all tests that call it to be skipped
- A bare marker (kind only, no modifier) is unconditional:
  - `# skip` always skips, `# flaky` always marks flaky, `# run` is a no-op
- A conditional marker requires a modifier (`if`/`unless`) and an expression
- Expressions are quoted strings with `${VAR}` interpolation or bare numbers:
  - `"${CI}"` — environment variable reference
  - `"literal"` — literal string
  - `"${HOST}:${PORT}"` — compound interpolation
  - `42` — bare number (compared as string)
- Bare variable identifiers (e.g. `CI`) are valid in markers
- Marker expression evaluation uses ENV-only lookup (`Arc<LayeredEnv>` — the layered host-plus-`.env` environment) — no frame variables or test-scope variables exist at marker-evaluation time (this scoping is specific to marker conditions; pure evaluation elsewhere, e.g. a `?` pure match inside a `pure fn`, does have a per-call capture frame)
- Truthiness: empty string or unset variable is false, any non-empty string is true
- `=` operator: evaluates both sides, returns the LHS value if it equals RHS exactly, empty string otherwise (unlike the shell literal-match operators `<=`/`!=`, which scan accumulated output for a substring; a marker compares against a complete value)
  - An empty RHS matches only an empty or unset LHS
  - For a substring or pattern check, use `?` (regex) instead: `expr ? value`
- `?` operator: evaluates LHS, compiles the regex pattern (with `${var}` interpolation), returns the match if found, empty string otherwise
- Modifier semantics:
  - `if` acts when the result is truthy
  - `unless` acts when the result is falsy
- Kind semantics:
  - `skip`: skips the test/effect when the condition is met
  - `run`: skips the test/effect when the condition is NOT met (inverse of `skip`)
  - `flaky`: marks the test as flaky — with `[flaky].max_retries > 0` in `Relux.toml`, a failing flaky test is retried from scratch with exponentially increasing tolerance timeouts (`base × m^(retry-1)`). With `max_retries = 0` (default), the marker is documentary only
- Multiple markers stack with AND semantics: all conditions must pass or the test is skipped
- When an effect is skipped, all tests depending on it are also skipped
- When a function is skipped, all tests that call it are also skipped
- `# flaky` propagates the same way skip does: a test is marked flaky if it, or any function or effect it reaches, has a `# flaky` marker whose condition applies

## Tests

- A test is the top-level unit of execution
- Tests are independent — no test depends on another test's execution or side effects
- Condition markers (`# skip/run/flaky ...`) are placed immediately before the `test` declaration
- Test structure (in order):
  1. Doc string (optional `"""..."""`)
  2. Preamble: `let` declarations (test-scoped variables) and [pure-match](08-pure-matching.md) statements (`<expr> = <pattern>` / `<expr> ? <pattern>`), interleaved; a `?` match binds `$n` into a preamble capture frame the later preamble items and overlays read
  3. `start` declarations (effect dependencies)
  4. `shell` blocks (test body) — each owns its own capture frame; the preamble frame is not inherited
  5. `cleanup` block (optional)
- Effects are instantiated and their shells are available before the test body runs
- A test succeeds if all match operations in all shell blocks pass
- A test fails if any match operation times out or any fail pattern matches
- A test is cancelled (a distinct outcome from failure) when execution is stopped before the test could finish: the test's own `~T` timeout fired, the suite-wide timeout fired, fail-fast cut sibling tests short, or the process received SIGINT. Cancelled outcomes exit nonzero in CI, exactly like failures, but they preserve the distinction that the test was not misbehaving

## Cleanup

- Cleanup blocks exist in both effects and tests
- Cleanup runs in a freshly spawned implicit shell, not in any existing shell
- Existing shells are terminated automatically by the runtime (cleanup is not for graceful shutdown)
- Cleanup is for external side effects: temp files, docker containers, log collection
- Any statement valid in a shell block is valid in a cleanup block
- Cleanup always executes, regardless of whether the test/effect passed or failed
- Cleanup failures are logged as warnings but do not change the test result
- Cleanup order: test cleanup runs first, then effect cleanups

## Execution Model

- The runtime discovers all `.relux` files, parses them, resolves imports and effect dependencies
- Tests are the entry points — only modules with `test` blocks are executed
- For each test:
  1. Resolve the effect dependency graph
  2. Run effects in topological order (reusing deduplicated instances)
  3. Execute the test body (shell blocks in declaration order)
  4. Run test cleanup
  5. Tear down effect instances (cleanup + shell termination)
- All shells within a test share the same test-scoped variables
- Only one shell is "active" at a time — statements execute sequentially, switching shells as blocks are entered
