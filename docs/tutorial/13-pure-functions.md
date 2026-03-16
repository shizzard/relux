# Pure Functions

[Previous: Effects and Dependencies](12-effects-and-dependencies.md)

In the [functions](08-functions.md) article, you learned to extract reusable test logic into named functions. Those functions work well for sequences of shell operations — sending commands, matching output, consuming prompts. But they have a limitation: they can only be called inside a `shell` block, because their bodies contain shell operators that need an active PTY session to run in.

That restriction becomes frustrating when you need to compute a value *before* a shell block exists. Suppose you write a helper that builds a URL:

```relux
fn format_url(host, port) {
    "${host}:${port}/api"
}
```

You want to use it in a test-scope `let` to prepare a configuration value before any shell starts:

```relux
test "connect to API" {
    let url = format_url("localhost", "8080")
    shell s {
        > curl ${url}
        <? ^200 OK$
        match_prompt()
    }
}
```

This does not work. `format_url` is a regular function, and regular functions require a shell context. The `let` on line 2 sits outside any shell block, so Relux has no shell to execute the function in.

The same problem appears in other places. You cannot call a regular function in an [effect](12-effects-and-dependencies.md#defining-an-effect)-scope `let`, and you cannot use one in an [overlay value](12-effects-and-dependencies.md#overlay-variables) for a [`need`](12-effects-and-dependencies.md#needing-an-effect) declaration. Anywhere outside a shell block, regular functions are off limits.

Pure functions solve this. Add the `pure` keyword before `fn`, and the function becomes shell-independent — callable from anywhere:

```relux
pure fn format_url(host, port) {
    "${host}:${port}/api"
}

test "connect to API" {
    let url = format_url("localhost", "8080")
    shell s {
        > curl ${url}
        <? ^200 OK$
        match_prompt()
    }
}
```

The test-scope `let` now works. The `pure` keyword tells Relux that this function operates on strings only and never touches a shell. In exchange for that restriction, it can be called from any expression context in the language.

Several [built-in functions](05-built-in-functions.md) are also available in pure context:

| Function                    | Description                       |
|-----------------------------|-----------------------------------|
| `trim(s)`                   | Strip leading/trailing whitespace |
| `upper(s)`                  | Convert to uppercase              |
| `lower(s)`                  | Convert to lowercase              |
| `replace(s, from, to)`      | Replace all occurrences           |
| `split(s, sep, idx)`        | Split and return the Nth element  |
| `len(s)`                    | String length                     |
| `uuid()`                    | Generate a UUID                   |
| `rand(n)` / `rand(n, mode)` | Generate random values            |
| `available_port()`          | Find a free TCP port              |
| `which(cmd)`                | Locate a command on `PATH`        |

## The `pure fn` syntax

A pure function definition looks like a regular function with the `pure` keyword prepended:

```relux
pure fn tag(key, value) {
    "${key}:${value}"
}
```

The body can contain:

- **String literals** with [variable interpolation](06-variables.md): `"${key}:${value}"`
- **Variable references**: a bare variable name as an expression
- **`let` declarations**: `let full = "${first} ${last}"`
- **Variable reassignment**: `x = upper(x)`
- **Calls to other pure functions and pure built-in functions**

The return value is the last expression in the body, the same rule as regular functions. A function ending with a `let` returns the assigned value. A function ending with a string literal returns that string.

Here is a pure function that uses `let` for an intermediate value:

```relux
pure fn build_greeting(first, last) {
    let full = "${first} ${last}"
    upper(full)
}
```

`build_greeting("jane", "doe")` returns `"JANE DOE"`. The `let` binds the concatenated name, then `upper()` — a pure built-in function — transforms it to uppercase. The result of `upper(full)` is the last expression, so it becomes the return value.

## What pure functions cannot do

The trade-off for calling pure functions anywhere is that their bodies cannot interact with a shell. Every shell operator is forbidden:

- Send operators: `>`, `=>`
- Match operators: `<=`, `<?`
- Timeout operators: `~`, `@`
- Fail pattern operators: `!?`, `!=`

If you try to use a shell operator inside a pure function, `relux check` reports the error:

```relux
pure fn bad() {
    > echo "side effect"
}
```

```
error: shell operator cannot be used in a pure function
```

Pure functions also cannot call regular (impure) functions or impure built-in functions. Calling a function that needs a shell from inside a function that has no shell makes no sense, so Relux rejects it:

```relux
fn impure_helper() {
    > echo "side effect"
    <? ^side effect$
}

pure fn bad() {
    impure_helper()
}
```

```
error: impure_helper/0 cannot be used in a pure function
```

The same applies to impure built-in functions like `match_prompt()`, `match_ok()`, `sleep()`, `log()`, and the `ctrl_*` family — they all require a shell:

```relux
pure fn bad() {
    match_prompt()
}
```

```
error: match_prompt/0 cannot be used in a pure function
```

These checks happen at compile time. You do not need to run the test to discover the mistake — `relux check` catches it before anything executes.

## Where you can call pure functions

The key advantage of pure functions is that they work in every expression context, not just inside shell blocks. Here is a summary of all the places you can call them:

**Inside a shell block**, just like regular functions:

```relux
pure fn greet(name) {
    "hello ${name}"
}

test "call pure function in shell" {
    shell s {
        let result = greet("world")
        > echo ${result}
        <? ^hello world$
    }
}
```

**In a test-scope `let`**, before any shell block:

```relux
pure fn tag(key, value) {
    "${key}:${value}"
}

test "pure function in test-scope let" {
    let label = tag("env", "test")
    shell s {
        > echo ${label}
        <? ^env:test$
    }
}
```

**In an effect-scope `let`**, to compute values during effect setup. The `let` sits outside the shell block, so only pure functions can be called here. Using the same `tag` function from above:

```relux
effect Config -> cfg {
    let label = tag("env", "production")
    shell cfg {
        > echo ${label}
        <? ^env:production$
        match_ok()
    }
}
```

**In overlay values for `need` declarations** — overlays are evaluated outside any shell, so pure functions are the only way to compute them dynamically:

```relux
pure fn make_label(name) {
    "label-${name}"
}

effect Labeled -> labeled {
    shell labeled {
        > echo $LABEL
        <? ^${LABEL}$
        match_ok()
    }
}

test "pure function in overlay" {
    need Labeled as labeled {
        LABEL = make_label("production")
    }
    shell labeled {
        > echo $LABEL
        <? ^label-production$
    }
}
```

**In other pure function bodies**:

```relux
pure fn wrap(s) {
    "[${s}]"
}

pure fn double_wrap(s) {
    wrap(wrap(s))
}
```

`double_wrap("hi")` returns `"[[hi]]"`. Pure functions compose naturally — each call evaluates and returns a string, which becomes the argument to the next call.

Condition markers can also call pure functions, but that is covered in a later article.

## What "pure" means in Relux

If you are familiar with functional programming, the word "pure" might suggest a function that is deterministic and free of side effects — calling it with the same arguments always produces the same result.

Relux uses a narrower definition. Two pure built-in functions violate the functional programming definition: `uuid()` and `rand()` return different values on each call. They are non-deterministic, yet Relux considers them pure.

In Relux, **"pure" means shell-independent**. A pure function does not read from or write to any PTY session. It operates on string values only and does not require an output buffer. This is a narrower guarantee than functional purity, but it is the guarantee that matters — it determines where a function can be called.

If a function does not use shell operators, it can be `pure fn`. If it sends commands or matches output, it must be a regular `fn`. That is the only distinction.

## Best practices

### Prefer `pure fn` when a function has no shell operators

You might write a helper as a regular function out of habit, because you first use it inside a shell block:

```relux
fn format_url(host, port) {
    "${host}:${port}/api"
}
```

This works fine in shell context. But later, when you want to use the same helper in a test-scope `let` or an overlay value, you discover it does not work — regular functions require a shell. You then have to go back and add the `pure` keyword.

Save yourself the trip: if a function body contains no shell operators, define it as `pure fn` from the start. It works in all the same places a regular function works, plus everywhere else.

### Extract complex interpolation into a pure function

When string interpolation gets deeply nested, the intent can become hard to read:

```relux
test "nested interpolation" {
    let host = "localhost"
    let port = "5432"
    let db = "myapp"
    shell s {
        > psql "postgres://${host}:${port}/${db}?sslmode=disable"
        <? ^connected$
        match_prompt()
    }
}
```

This is manageable, but as the string grows — multiple parameters, conditional segments, repeated patterns — readability suffers. A pure function gives the construction a name and keeps the test body focused on intent:

```relux
pure fn pg_url(host, port, db) {
    "postgres://${host}:${port}/${db}?sslmode=disable"
}

test "extracted into pure function" {
    let host = "localhost"
    let port = "5432"
    shell s {
        let url = pg_url(host, port, db)
        > psql "${url}"
        <? ^connected$
        match_prompt()
    }
}
```

## Try it yourself

Write a pure function `format_config(app, env, port)` that returns a structured string like `"app=myapp env=prod port=8080"`.

1. Call it from a test-scope `let` and verify the result by echoing it in a shell block
2. Call it directly inside a shell block and use the return value in a send
3. Write a second pure function `format_config_upper(app, env, port)` that calls `format_config` and passes the result through `upper()`. Verify it returns `"APP=MYAPP ENV=PROD PORT=8080"`.

---

Next: [Cleanup](14-cleanup.md) — teardown blocks for effects and tests
