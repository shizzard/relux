# Variables

[Previous: Built-in Functions](05-built-in-functions.md)

So far, every value in the tests has been hardcoded — the command to send, the string to match, the exit code to check. That works for small examples, but real tests need to capture output, pass values between operations, and avoid repeating the same string in multiple places. Variables solve all of these problems.

All values in the Relux DSL are strings. There are no integers, booleans, lists, or other types — just strings. Every variable holds a string, every expression produces a string, every function argument is a string. This is a deliberate design choice that keeps the language simple: when your job is sending text to a shell and matching text coming back, strings are the only type you need.

Variable names must start with a lowercase letter or an underscore, followed by any combination of letters (upper or lower), digits, and underscores. Both `snake_case` and `camelCase` are valid names. Names starting with an uppercase letter are reserved for effects, which you will learn about in a later article.

## Declaring variables with `let`

The `let` keyword declares a variable and optionally binds it to a value:

```relux
let name = "relux"
let count = 3
let empty
```

The first form, `let name = "value"`, is the most common. It declares a variable and sets its value.

The second form, `let count = 3`, shows that literal numbers can go unquoted. Since all values are strings, `3` is stored as the string `"3"` — the quotes are optional for numbers. This is why [built-in function](05-built-in-functions.md) calls like `match_exit_code(1)` work without quotes around the argument.

The third form, `let empty`, declares a variable with no value. It defaults to the empty string `""`. This is useful when you want to declare a variable early and assign it later.

## String interpolation with `${var}`

Once declared, a variable is referenced with the `${var}` syntax. Relux replaces each `${var}` with the variable's current value before the operation executes:

```relux
test "interpolation basics" {
    shell s {
        let greeting = "hello"
        let target = "world"
        > echo "${greeting} ${target}"
        <= hello world
        match_ok()
    }
}
```

Interpolation works everywhere — in send operators, in literal match patterns, and in string expressions passed to functions.

If you reference a variable that has not been declared, it interpolates to the empty string — no error, no warning. The text simply disappears:

```relux
test "undefined variable is empty" {
    shell s {
        > echo "before${nonexistent}after"
        <= beforeafter
        match_ok()
    }
}
```

This is a deliberate design choice. It makes environment variable access seamless (you don't know ahead of time whether a host variable exists), but it also means a typo in a variable name will silently produce an empty string rather than an error.

## Everything has a value

Every expression in Relux produces a string value. This means `let` can capture the result of any expression — not just string literals.

Here is what each expression you have seen so far returns:

| Expression | Returns |
|---|---|
| `"hello"` | The string itself: `hello` |
| `> command` | The interpolated text that was sent |
| `=> text` | The interpolated text that was sent |
| `<= pattern` | The pattern that was matched |
| `match_prompt()` | The prompt string |
| `match_ok()` | The prompt string |
| `match_not_ok()` | The prompt string |
| `match_exit_code(code)` | The prompt string |
| `ctrl_c()`, `ctrl_d()`, etc. | Empty string |
| `log(message)` | The message |
| `annotate(text)` | The annotation text |

Since every expression returns a value, you can capture any of them with `let`:

```relux
test "let from expressions" {
    shell s {
        > echo "status=ok"
        let matched = <= status=ok
        match_prompt()
        > echo "I matched: ${matched}"
        <= I matched: status=ok
        match_ok()
    }
}
```

The `let matched = <= status=ok` line does two things at once: it performs the [literal match](03-send-match-and-logs.md) against the output buffer and stores the matched pattern text in the variable `matched`.

## Reassignment

Once a variable is declared with `let`, you can change its value using the assignment operator `=` — without the `let` keyword:

```relux
test "reassignment" {
    shell s {
        let x = "before"
        > echo ${x}
        <= before
        match_prompt()

        x = "after"
        > echo ${x}
        <= after
        match_ok()
    }
}
```

The variable must have been declared with `let` first. Assigning to an undeclared variable is a runtime error:

```relux
test "assign without let fails" {
    shell s {
        # This line will cause a runtime error:
        # "assignment to undeclared variable `x`"
        x = "oops"
    }
}
```

You can reference the variable's current value on the right-hand side of an assignment — the old value is read before the new one is written:

```relux
test "self-referencing assignment" {
    shell s {
        let x = "foo"
        x = "${x}bar"
        > echo ${x}
        <= foobar
        match_ok()
    }
}
```

## Escaping: the `$$` literal

Since `${...}` triggers variable interpolation, you need an escape when you want a literal dollar sign. Relux uses `$$` — two dollar signs produce one literal `$` in the output.

This matters most when you need to send the literal text `${...}` to the shell — for example, to reference a shell variable using brace syntax:

```relux
test "shell-side brace expansion via dollar escape" {
    shell s {
        > MY_SERVICE=api && echo "$${MY_SERVICE}_port"
        <= api_port
        match_ok()
    }
}
```

Without the `$$`, writing `> echo "${MY_SERVICE}_port"` would trigger Relux interpolation — Relux would look up a variable named `MY_SERVICE`, find nothing, and send `echo "_port"` to the shell. The shell would never see the `$`.

With `$$`, Relux produces the literal text `${MY_SERVICE}_port`, sends it to the shell, and the shell performs its own variable expansion.

You can mix escapes with interpolation in the same expression:

```relux
test "dollar escape with variable interpolation" {
    shell s {
        let name = "USD"
        > echo "currency: $$${name}"
        <= currency: $USD
        match_ok()
    }
}
```

`$$` produces `$`, and `${name}` produces `USD`. The shell receives `echo "currency: $USD"`.

## Scoping

Variables in Relux exist at one of two levels: **test scope** and **shell scope**.

**Test scope** — variables declared outside any `shell` block, directly inside a `test` block. These are visible to all shells in the test:

```relux
test "test-level variable shared across shells" {
    let shared = "from-test"

    shell a {
        > echo "a=${shared}"
        <= a=from-test
        match_ok()
    }

    shell b {
        > echo "b=${shared}"
        <= b=from-test
        match_ok()
    }
}
```

Both `a` and `b` can see `shared` because it was declared at test level.

**Shell scope** — variables declared inside a `shell` block. These live in that shell's scope and are not visible to other shells:

```relux
test "shell-scoped variable" {
    shell a {
        let local = "only-in-a"
        > echo ${local}
        <= only-in-a
        match_ok()
    }

    shell b {
        > echo "local='${local}'"
        <= local=''
        match_ok()
    }
}
```

The variable `local` is declared inside shell `a`. When shell `b` tries to reference it, `${local}` interpolates to the empty string — it simply does not exist in `b`'s scope.

### Shadowing

A shell-scoped variable with the same name as a test-scoped variable **shadows** it within that shell. The test-scoped value is unchanged and remains visible in other shells:

```relux
test "shadowing" {
    let x = "test-level"

    shell a {
        let x = "shadowed-in-a"
        > echo ${x}
        <= shadowed-in-a
        match_ok()
    }

    shell b {
        > echo ${x}
        <= test-level
        match_ok()
    }
}
```

Shell `a` declares its own `x`, which shadows the test-level `x` inside `a`. Shell `b` still sees the original test-level value.

## Environment variables

Host environment variables — the ones you see with `env` or `printenv` in your terminal — are accessible through the same `${VAR}` syntax as Relux variables:

```relux
test "access host environment variable" {
    shell s {
        > echo ${HOME}
        <= /
        match_ok()
    }
}
```

`${HOME}` is not a Relux variable — no `let` declared it. Relux checks its own variables first (shell scope, then test scope), and when it finds nothing, it falls through to the host process environment. This works for any environment variable set in the process that runs `relux`.

Environment variables are **global** — they are visible in every test, every shell block, every scope. And they are **immutable** — you cannot reassign them from within the Relux DSL.

A `let` with the same name creates a Relux variable that *shadows* the environment variable. However, Relux variable names must start with a lowercase letter or underscore, so uppercase environment variables like `HOME` or `PATH` cannot be shadowed — there is no valid Relux variable name that matches them. They are always readable and never obscured.

Environment variables that happen to use a compatible naming scheme (lowercase, snake_case) can be shadowed. In that case, the Relux variable takes priority within its scope, and the environment variable remains accessible in scopes where no shadow exists.

### Relux environment variables

Relux injects several variables into every test run. These are real environment variables — every spawned shell process inherits them, so they are accessible both through `${VAR}` in the Relux DSL and through standard shell expansion (e.g., `echo $__RELUX_RUN_ID`) inside the shell itself. You can pass them to scripts, programs, or any command launched from within the test.

- `${__RELUX_RUN_ID}` — the unique identifier for the current test run
- `${__RELUX_RUN_ARTIFACTS}` — the path to the run's `artifacts/` subdirectory (inside the run directory under `relux/out/`). This is a good place to store files related to the test run: generated configs, temporary databases, downloaded fixtures, or any other artifacts that should be preserved alongside the [test logs](03-send-match-and-logs.md).
- `${__RELUX_SHELL_PROMPT}` — the configured shell prompt string
- `${__RELUX_SUITE_ROOT}` — the absolute path to the project root (where `Relux.toml` lives)
- `${__RELUX_TEST_ROOT}` — the absolute path to the directory containing the current test file

## Try it yourself

Write a test with two shells and the following behavior:

1. Declare a test-level variable `tag` with a value like `"build-42"`.
2. In the first shell, use `$$` to set a *shell-side* environment variable (with `export`) whose value comes from the Relux `tag` variable. Verify it was set by echoing it back through `$$`.
3. In the second shell, verify that the shell-side export from the first shell is *not* visible (shells are independent processes), but the Relux `tag` variable *is* visible (test-scoped variables are shared).
4. Back in the first shell, declare a shell-scoped `let tag` that shadows the test-level one. Verify the shadow is in effect, then switch to the second shell and verify the original test-level value is unchanged.

This exercise combines test-scoped variables, shell independence, `$$` escaping, and shadowing — all the pieces from this article.

---

Next: [Regex Matching](07-regex-matching.md) — match output with regular expressions and extract captured values
