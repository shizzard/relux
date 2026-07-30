# Functions

`fn` runs in the caller's shell context. `pure fn` evaluates to a string with no shell context.

## `fn`

```relux
fn start_service(name) {
    !? ERROR
    > start-${name}
    <? ready
}
```

- Snake_case name (parse-enforced).
- No shell of its own -- uses the caller's active shell.
- Shell operators (`>`, `=>`, `<?`, `<=`, `!?`, `!=`, multimatch, timeouts) are valid.
- Impure BIFs (`match_ok`, `ctrl_c`, etc.) are valid.
- Per-frame state -- timeout, fail-pattern slot, captures, `let` bindings -- is frame-scoped and reverts when the call returns. The caller's prior values are restored as if the call never touched them.
- The barrier is bidirectional. The function does NOT see the caller's `let` bindings either; the only inputs that cross the boundary are the explicit arguments. Pass values in as arguments, return them as the function's last expression.
- Caller-shared state -- the PTY buffer cursor, shell-side env vars (`export`s), working directory, running processes -- persists. The function operates on the caller's actual shell.
- Return value: last expression in the body. Implicit `""` if the body ends on a non-expression statement.
- Callable only inside a shell block (since there is no shell otherwise).

## `pure fn`

```relux
pure fn compose_url(host, port) {
    "http://${host}:${port}/api"
}
```

- Snake_case name.
- May contain: `let`, reassignment, expressions, pure BIF calls, calls to other pure functions.
- Cannot contain: shell operators, timeouts, impure BIFs, calls to `fn`.
- Callable from: condition markers, overlay expressions, `let` RHS, shell-block expressions.
- "Pure" means shell-independent, not deterministic -- `uuid()`, `rand()`, `sleep()`, `which()` are all allowed.

## Arguments and returns

- All arguments are strings.
- No type annotations.
- Return is the value of the last expression in the body.
- Captures (`$1`, `$2`) do NOT carry into a `fn`. The function opens its own capture frame -- `$1` is empty inside the body until the function's own `<?` populates it. Captures the function produces are likewise not visible to the caller after the call returns. To pass a captured value across the boundary, pass it as a function argument or return it from a function.

## Arity-based dispatch

- Every function is identified by its `(name, arity)` pair. Same name with a different number of parameters resolves to a different function -- no overloading by type (everything is a string anyway), only by arity.
- Applies to both `fn` and `pure fn`, and to BIFs (e.g. `match_not_ok()` and `match_not_ok(code)` are distinct).
- There is no conditional branching inside a function body. Arity dispatch is the language's mechanism for default arguments: define each arity separately and have the smaller ones delegate to the larger.

## Pitfalls and best practices

### Leave the caller's shell clean

The function operates on the caller's PTY. The buffer cursor, working directory, and any shell-side state the function touched survive after the call returns. A function that leaves unconsumed output forces every caller to know what's still in the buffer; the next match in the caller picks up the leftover and misbehaves.

After the last command, consume the prompt and verify the exit code with `match_ok()` (or the appropriate `match_not_ok` variant).

Don't:

```relux
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
}
```

Do:

```relux
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
    match_ok()
}
```

### Use `pure fn` for value derivation

If a helper has no shell I/O, mark it `pure`. It composes inside `let`, interpolations, overlay expressions, and marker conditions -- places where `fn` cannot be used.

Don't:

```relux
fn compose_url(host, port) {
    "http://${host}:${port}/api"
}
```

Do:

```relux
pure fn compose_url(host, port) {
    "http://${host}:${port}/api"
}
```

### Use arity-based dispatch for default arguments

Relux has no conditional branching inside a function body. To express defaults, define each arity as its own function and have the smaller ones delegate to the larger one. The default values live exactly once, in the function that supplies them.

Don't: try to fake defaults by checking for an empty string inside the body (the language has no `if` -- this isn't possible).

Do:

```relux
fn http_request(expected_code, url) {
    http_request(expected_code, url, "GET")
}

fn http_request(expected_code, url, method) {
    http_request(expected_code, url, method, "")
}

fn http_request(expected_code, url, method, req_body) {
    let response_filename := curl(url, method, req_body)
    http_match_code(expected_code)
    match_ok()
    response_filename
}
```

Each call site picks the shortest form that fits: `http_request(200, "/health")` uses GET with no body; `http_request(201, "/users", "POST", "{...}")` reaches the full form directly. Changing the default method or body is a one-line edit in the delegating arity.

### Don't depend on shell-side state set by the caller (or vice versa)

Callee and caller share one PTY, so `export`s, cwd, and process state are mutually visible. A function that reads `$FOO` from the caller silently breaks the moment a new caller forgets to set it; a caller that reads a function's `export`s couples to its internals. Pass values as arguments and return them as values; reserve shell-side state for things that genuinely need to persist across operations on the same shell.

Don't:

```relux
fn check_my_var() {
    > echo $$MY_VAR
    <? ^expected$
    match_ok()
}

test "fragile: relies on caller setting MY_VAR" {
    shell s {
        > export MY_VAR=expected
        match_ok()
        check_my_var()
    }
}
```

Do:

```relux
fn check_env_var(name, expected_regex) {
    > echo "$$${name}"
    <? ${expected_regex}
    match_ok()
}

test "explicit: env var name and expected pattern passed in" {
    shell s {
        > export MY_VAR=expected
        match_ok()
        check_env_var("MY_VAR", "expected")
    }
}
```

## See also

- [statements](statements.md) -- read if you need `let`, captures, or where shell ops are valid
- [bifs](bifs.md) -- read if you need pure vs impure built-ins
- [markers](markers.md) -- read if you need `pure fn` usage in marker conditions, or if the function calls an external command (`docker`, `kubectl`, `jq`, etc.) that may not be on every target host -- guard it with a marker so callers inherit the skip
