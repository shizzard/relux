# Functions

[Previous: Regex Matching](07-regex-matching.md)

The previous articles covered the core toolkit for interacting with a shell: [sending commands](03-send-match-and-logs.md), [matching output](04-the-output-buffer.md), [calling built-in functions](05-built-in-functions.md), [storing values in variables](06-variables.md), and [extracting data with regex](07-regex-matching.md). With these tools you can write any test — but you will quickly find yourself repeating the same sequences of operations across tests. A health check that sends an HTTP request and verifies the status code. A login sequence that types a username, a password, and waits for a prompt. A cleanup step that kills a background process.

Relux lets you extract these sequences into **functions** — named, reusable blocks of test logic that you define once and call from any test:

```relux
fn check_status(url) {
    check_status(url, "")
}

fn check_status(url, params) {
    check_status(url, params, 200)
}

fn check_status(url, params, expected) {
    > curl -s -o /dev/null -w "%{http_code}\n" "${url}${params}"
    <? ^${expected}$
    match_ok()
}
```

Three definitions of the same function with different numbers of parameters. You can call it three different ways:

```relux
check_status("http://localhost:8080/health")
check_status("http://localhost:8080/users", "?page=1")
check_status("http://localhost:8080/admin", "", 403)
```

One name, three levels of detail.

## Defining a function

A function definition starts with the `fn` keyword, followed by a name, a parameter list in parentheses, and a body in braces:

```relux
fn greet() {
    > echo "hello from fn"
    <? ^hello from fn$
    match_ok()
}
```

This defines a function called `greet` that takes no arguments. Its body sends a command, matches the output, and consumes the prompt — the same operators you use directly in shell blocks.

Function names must be `snake_case` — lowercase letters, digits, and underscores. This is enforced by the parser. If you try to define a function with an uppercase name like `CheckStatus`, Relux will reject the file.

Parameters go inside the parentheses, separated by commas:

```relux
fn say(msg) {
    > echo "${msg}"
    <? ^${msg}$
    match_ok()
}

fn add_label(prefix, value) {
    > echo "${prefix}: ${value}"
    <? ^${prefix}: ${value}$
    match_ok()
}
```

## Calling a function

If you have been following the series, you already know how to call functions — the syntax is the same as for [built-in functions](05-built-in-functions.md). Write the name, followed by arguments in parentheses:

```relux
test "call function with multiple arguments" {
    shell s {
        add_label("status", "ok")
    }
}
```

Function calls can only appear inside `shell` blocks. This makes sense once you understand the execution model: a function's body contains shell operators like `>` and `<?` that need an active shell to operate on.

## Arity-based dispatch

You saw [arity](05-built-in-functions.md) with built-in functions — `match_not_ok()` and `match_not_ok(code)` are two separate functions that share a name. The same mechanism works for user-defined functions. Relux identifies every function by its `(name, arity)` pair, so you can define multiple versions with different parameter counts:

```relux
fn greet() {
    > echo "hello"
    <? ^hello$
    match_ok()
}

fn greet(name) {
    > echo "hello, ${name}"
    <? ^hello, ${name}$
    match_ok()
}

fn greet(name, title) {
    > echo "hello, ${title} ${name}"
    <? ^hello, ${title} ${name}$
    match_ok()
}

test "arity dispatch" {
    shell s {
        greet()
        greet("alice")
        greet("alice", "Dr.")
    }
}
```

Each call resolves to the definition with the matching number of parameters.

Arity dispatch is more powerful than it might first appear. Relux has no built-in branching or conditionals in function bodies. You cannot write `if params == ""` to check whether an argument was provided. Arity dispatch is the language's answer to default parameters: instead of one function that checks for empty strings internally, you write multiple definitions at different arities and have the simpler ones delegate to the fuller one.

The `check_status` function from the opening is the canonical example. Each definition defaults exactly one parameter and delegates to the next arity up:

- `check_status/1` fills in an empty query string and delegates to `check_status/2`
- `check_status/2` fills in the default expected status code of `200` and delegates to `check_status/3`
- `check_status/3` does the actual work

This progressive chain means each default value appears exactly once. If the default expected code ever changes from `200` to something else, you update one line in `check_status/2`. No value is duplicated across definitions.

Notice that number literals like `200` and `403` are unquoted — since all values are strings, Relux accepts bare numbers and stores them as strings.

## The caller's shell

A function does not get its own shell. When you call a function from inside a shell block, the function's body executes in **the caller's shell** — the same PTY session that the call site is running in. Every `>` sends to that shell, every `<?` matches against that shell's output buffer.

This means a function can see everything the caller has done to the shell:

```relux
fn check_shell_var() {
    > echo $$MY_VAR
    <? ^caller_state$
    match_ok()
}

test "function executes in caller shell" {
    shell s {
        > export MY_VAR=caller_state
        match_ok()
        check_shell_var()
    }
}
```

The test exports an environment variable in shell `s`, then calls `check_shell_var()`. The function runs in the same shell — it reads `MY_VAR` and finds `caller_state`. There is no argument passing or special plumbing; the function simply shares the caller's PTY session.

## Scoping

Functions share the caller's shell, but they do **not** share the caller's [variables](06-variables.md). The isolation goes both ways: the function cannot see the caller's variables, and the caller cannot see the function's variables. The only variables available inside a function are its own parameters, anything it declares with `let`, and environment variables.

```relux
fn try_read_secret() {
    > echo "secret='${secret}'"
    <? ^secret=''$
    match_ok()
}

test "function cannot see caller variables" {
    shell s {
        let secret = "caller-only"
        try_read_secret()
    }
}
```

The caller declares `secret`, but inside `try_read_secret()` the variable `${secret}` resolves to the empty string. It does not exist in the function's scope. If a function needs a value from the caller, it must be passed as an argument.

```relux
fn say(msg) {
    > echo "${msg}"
    <? ^${msg}$
    match_ok()
}

test "function variables do not leak to caller" {
    shell s {
        say("test")
        > echo "msg='${msg}'"
        <? ^msg=''$
    }
}
```

After `say("test")` returns, `${msg}` in the caller's scope resolves to the empty string. The parameter `msg` existed only inside the function.

```relux
fn shadow_x() {
    let x = "from-function"
    > echo "inside: x=${x}"
    <? ^inside: x=from-function$
    match_ok()
}

test "function let does not mutate outer variable" {
    shell s {
        let x = "outer"
        shadow_x()
        > echo "x=${x}"
        <? ^x=outer$
    }
}
```

The function's `let x` creates a local variable within the function's own scope. The caller's `x` remains `"outer"` after the call.

The mental model is simple: **scope isolation is bidirectional.** The only shared state is the shell itself — the PTY session, the running processes, the shell-side environment variables.

## Return values

The [variables](06-variables.md) article introduced the idea that every expression produces a value. Functions follow the same principle: a function's return value is the value of its last expression. If the caller does not capture it, the return value is silently discarded.

```relux
fn make_label(prefix, value) {
    "${prefix}:${value}"
}

test "expression as return value" {
    shell s {
        let label = make_label("key", "val")
        > echo "${label}"
        <? ^key:val$
    }
}
```

A bare string expression at the end of the body becomes the return value. A `let` statement also produces a value — the value it assigns — so a function ending with `let` returns that value too. An empty function returns the empty string.

A common pattern is a function that runs a command, matches the output with `<?`, and returns a captured value:

```relux
fn capture_version() {
    > echo "version=3.2.1"
    <? ^version=(.+)$
    let ver = ${1}
    match_ok()
    ${ver}
}

test "capture return value from match" {
    shell s {
        let ver = capture_version()
        > echo "got=${ver}"
        <? ^got=3.2.1$
    }
}
```

The regex match sets `${1}` to `3.2.1`. The function saves the capture, cleans up the shell with `match_ok()`, and returns the saved value as the last expression.

As later articles introduce new kinds of expressions, they will note what value each one returns — following the same pattern as the everything-has-a-value table.

## Functions calling functions

Functions can call other functions. Return values chain naturally — each function captures the result of the one it called and builds on it:

```relux
fn make_prefix(tag) {
    "[${tag}]"
}

fn log_msg(tag, msg) {
    let pfx = make_prefix(tag)
    > echo "${pfx} ${msg}"
    <? ^\[${tag}\] ${msg}$
    match_ok()
}

test "nested function uses helper return value" {
    shell s {
        log_msg("WARN", "check this")
    }
}
```

`log_msg` calls `make_prefix` to build a formatted tag, then uses the result in a send. Each function has its own scope, so `tag` in `make_prefix` and `tag` in `log_msg` do not collide — even though they happen to have the same name.

Return values can chain through multiple levels:

```relux
fn depth_a(x) {
    let val = depth_b(x)
    "${val}-a"
}

fn depth_b(x) {
    let val = depth_c(x)
    "${val}-b"
}

fn depth_c(x) {
    "${x}-c"
}

test "nested function return value chains" {
    shell s {
        let result = depth_a("root")
        > echo "${result}"
        <? ^root-c-b-a$
    }
}
```

`depth_a` calls `depth_b`, which calls `depth_c`. Each function appends its suffix to the result. The final value, `root-c-b-a`, traces the entire call chain.

## Best practices

### Captures do not survive function calls

You might call a function that internally uses `<?` and expect the capture groups (`${1}`, `${2}`, ...) to be available in the caller afterward. This seems reasonable — the function ran a regex match, and captures are normally available after `<?`.

But captures are part of the variable scope. When a function returns, its entire scope — including captures — is discarded. The caller's captures are restored to whatever they were before the call:

```relux
fn extract_port() {
    > echo "port=8080"
    <? ^port=(\d+)$
    # The last expression is match_ok(), whose return value is the
    # prompt string — not the captured port number.
    match_ok()
}

test "captures do not survive function calls" {
    shell s {
        # Wrong — ${1} holds the caller's capture state, not the function's:
        extract_port()
        > echo "port=${1}"
        <? ^port=8080          # ${1} is empty

        # Also wrong — the return value is the prompt string, because
        # match_ok() is the last expression in extract_port():
        let result = extract_port()
        > echo "result=${result}"
        <? ^result=8080        # result is the prompt, not "8080"
    }
}
```

The fix is to design the function to explicitly return what you need. Save the capture to a local variable before calling `match_ok()`, then return that variable as the last expression:

```relux
fn extract_port() {
    > echo "port=8080"
    <? ^port=(\d+)$
    let port = ${1}
    match_ok()
    ${port}
}
```

Now `let port = extract_port()` in the caller gives you `"8080"`.

This is consistent with the scoping model: functions cannot modify the caller's variable state. Return values are the explicit, reliable channel for passing data back.

### Leave the shell clean

When a function interacts with the shell — sending commands and matching output — it should leave the shell in a known state before returning. That means: consume the prompt and verify the exit code with `match_ok()` (or the appropriate `match_not_ok` variant) after the last command.

```relux
# Leaves the shell in an unknown state — the caller must
# know what output is left in the buffer:
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
}

# Leaves the shell clean — prompt consumed, exit code verified:
fn check_server() {
    > curl -s http://localhost:8080/health
    <= healthy
    match_ok()
}
```

A function that leaves unconsumed output or an unchecked exit code forces every caller to clean up after it. That coupling is invisible and fragile — it works until someone adds a new caller that forgets, or the function's output changes slightly. Close every shell interaction with a clean handoff.

### Do not rely on a shared shell state

The caller and the function share a shell session. This means the function can read shell-side environment variables set by the caller, and the caller can read shell-side state left behind by the function. Both directions are tempting shortcuts — and both lead to brittle tests.

A function cannot predict the shell state of all its callers. Some callers have not been written yet. If a function depends on a shell-side variable that the caller must set beforehand, the requirement is invisible — nothing in the function signature or call site reveals it. Pass the value as a parameter instead.

In the other direction, a caller that depends on shell-side state set by a function is coupled to the function's implementation details. If the function's internals change — a different variable name, a different order of commands — the caller silently breaks.

If you genuinely cannot avoid relying on shared shell state, make it explicit with a comment at both the definition and call site explaining the dependency. But first, consider whether a parameter or return value would work instead.

### Keep functions small

A function runs in the caller's shell, so a long function body means a long sequence of sends and matches executing in someone else's shell session. When something fails halfway through a 30-line function, the error points to a line inside the function — but understanding *why* it failed requires knowing what the caller's shell looked like at the time of the call.

Prefer small functions that do one thing: check a status code, verify a service is running, send a login sequence. If you find a function growing beyond a handful of operations, consider splitting it into smaller pieces — so each has a clear, narrow purpose.

## Try it yourself

Write a function `run_and_capture` with two arities:

- `run_and_capture(cmd)` — runs a shell command and returns the first line of its output. Leaves the shell clean.
- `run_and_capture(cmd, pattern)` — same, but uses a custom regex pattern instead of matching any line. The one-argument version delegates to this one.

Then write a test that exercises both arities and verifies the return values.

---

Next: [Timeouts](09-timeouts.md) — control how long Relux waits for output, from individual matches to entire test suites
