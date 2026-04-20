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
- Reassignment (`x = expr`) mutates an existing variable from an outer scope
- Environment variables from the host process are available as pre-set variables in all scopes (read-only — `let` creates a shadow, not a modification of the process environment)
- Regex capture groups (`$1`, `$2`, ...) are set after a `<?` match and remain in scope until overwritten by the next `<?`

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
- Can only contain: `let` declarations, variable reassignment, and expressions
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
- `start Effect as Alias { KEY = expr }` provides an overlay that remaps the caller's environment into the dependency's environment
  - The shorthand form `KEY` (without `= expr`) is equivalent to `KEY = KEY`
- Effects inherit the full parent environment — overlay entries override specific keys
- Effect instance identity is determined by `(effect-name, evaluated overlay restricted to expect-declared vars)`:
  - Same identity tuple = same instance (deduplicated, reused)
  - Different identity tuple = separate instances
- When a test or effect starts the same effect multiple times with the same evaluated overlay, only one instance is created
- Exposed shells are accessed via dot notation: `shell Alias.shell_name { ... }`
- Exposed variables are accessed via dot notation in interpolation: `${Alias.var_name}`
- Exposed variables are only accessible in shell contexts (runtime); test-level and effect-level `let` bindings cannot reference them (purity violation — `let` is evaluated at resolve time, before effects are started)
- Exposed variables are read-only from the caller's perspective
- For composed effects, `expose` can re-export a dependency's shell or variable: `expose shell Dep.shell as public_name`, `expose var Dep.port as db_port`
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
- Expression evaluation uses ENV-only lookup (`Arc<Env>`) — no frame variables or test-scope variables exist at evaluation time
- Truthiness: empty string or unset variable is false, any non-empty string is true
- `=` operator: evaluates both sides, returns the LHS value if LHS equals RHS, empty string otherwise
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

## Tests

- A test is the top-level unit of execution
- Tests are independent — no test depends on another test's execution or side effects
- Condition markers (`# skip/run/flaky ...`) are placed immediately before the `test` declaration
- Test structure (in order):
  1. Doc string (optional `"""..."""`)
  2. `let` declarations (test-scoped variables)
  3. `start` declarations (effect dependencies)
  4. `shell` blocks (test body)
  5. `cleanup` block (optional)
- Effects are instantiated and their shells are available before the test body runs
- A test succeeds if all match operations in all shell blocks pass
- A test fails if any match operation times out or any fail pattern matches

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
