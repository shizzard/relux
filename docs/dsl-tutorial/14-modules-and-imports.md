# Modules and Imports

[Previous: Cleanup](13-cleanup.md)

The previous articles built up everything you need to test programs thoroughly, but every example so far has lived in a single file. As a test suite grows, you end up with the same helper functions and effect definitions duplicated across test files. Change the startup sequence for a service, and you are editing the same code in five different places.

Relux solves this with modules and imports. Every `.relux` file is a module. You put shared code in the `lib/` directory, and test files import what they need.

Here is a library module at `lib/utils/greeter.relux`:

```relux
fn greet(name) {
    > echo "hello ${name}"
    <? ^hello ${name}$
    match_prompt()
}

fn farewell(name) {
    > echo "goodbye ${name}"
    <? ^goodbye ${name}$
    match_prompt()
}

effect StartGreeter -> svc {
    shell svc {
        > export GREETER_STATUS=running
        match_ok()
    }
}
```

Two functions and an effect. Now a test file can pull in what it needs:

```relux
import utils/greeter

test "say hello" {
    shell s {
        greet("alice")
    }
}
```

The `import` line brings everything from the library module into scope. Change `greet` in one place, and every test that imports it picks up the change.

## Every file is a module

A `.relux` file is automatically a module. There is no special declaration — the file's path relative to the project root determines its module identity. 

A file at `lib/utils/greeter.relux` has the module path `utils/greeter`. The `.relux` extension and the `lib/` prefix are stripped; what remains is the module path you use in `import` statements.

## Project structure

A Relux project has two top-level directories under the project root (where [`Relux.toml`](02-getting-started.md) lives):

- **`lib/`** — shared modules containing functions, pure functions, and effects. These are never run directly as tests.
- **`tests/`** — test files. Each `.relux` file here is discovered and executed by `relux run`.

Import paths always resolve from `lib/`. When you write `import utils/greeter`, Relux looks for `lib/utils/greeter.relux`. It does not matter where the importing file is — a test at `tests/deep/nested/test.relux` still imports `utils/greeter` the same way. This keeps import statements consistent across the entire project: the same module path always means the same file.

## Selective imports

The most explicit form of import names exactly which items you want from a module:

```relux
import utils/greeter { greet }
```

This brings the `greet` function into scope. The module `utils/greeter` may export other things — in this case it also defines `farewell` — but only `greet` is available in this file. Calling `farewell` would be an error.

You can import multiple items from the same module by listing them — both functions and [effects](11-effects-and-dependencies.md):

```relux
import utils/greeter { greet, StartGreeter }
```

This pulls in the `greet` function and the `StartGreeter` effect. Functions use `snake_case` names, effects use `CamelCase` — the naming convention is how Relux (and the reader) can tell them apart at a glance.

Trailing commas are allowed:

```relux
import utils/greeter {
    greet,
    StartGreeter,
}
```

## Wildcard imports

If you want everything a module exports, leave out the braces:

```relux
import utils/greeter
```

This brings all exported names into scope — both `greet` and `farewell`, as well as the `StartGreeter` effect in this case:

```relux
import utils/greeter

test "wildcard import makes all functions available" {
    need StartGreeter
    shell s {
        greet("world")
        farewell("world")
    }
}
```

Wildcard imports are convenient for small, focused modules where you know you want everything. For larger modules, selective imports make the dependencies clearer.

## Aliases

Sometimes an imported name collides with something in your file, or you simply want a shorter or more descriptive name. The `as` keyword renames an import:

```relux
import utils/greeter { greet as hello, farewell as bye }
```

Now `hello` and `bye` are the callable names — the originals `greet` and `farewell` are not in scope. Aliases work for effects too:

```relux
import utils/greeter { StartGreeter as Svc }

test "aliased effect" {
    need Svc as svc
    shell svc {
        > echo $$GREETER_STATUS
        <? ^running$
    }
}
```

There is one rule: **aliases must preserve casing kind**. A `snake_case` function must be aliased to another `snake_case` name. A `CamelCase` effect must be aliased to another `CamelCase` name. Aliasing `greet as Hello` or `StartGreeter as start_greeter` is a compile error — the casing convention is structural, not cosmetic.

## What gets exported

A module exports everything it defines:

- All `fn` definitions
- All `pure fn` definitions
- All `effect` definitions

Test definitions are not exported. A `test` block is local to the file it appears in — you cannot import a test from another module.

There is no visibility modifier. If a function exists in a module, it is exported. If you do not want something exported, the only option is to not put it in a shared `lib/` module — though in practice this is rarely a concern. Functions in library modules are there to be shared.

## Try it yourself

1. Create a library module at `lib/helpers.relux` with two functions: `check_running()` that echoes "running" and matches it, and `check_stopped()` that echoes "stopped" and matches it. Add an effect `StartWorker` that exports a shell and sets an environment variable `WORKER_STATUS=active`.

2. Write a test file `tests/selective_test.relux` that selectively imports only `check_running` and `StartWorker`. Write one test that calls `check_running()` in a shell, and another that needs `StartWorker` and verifies the environment variable.

3. Write a second test file `tests/wildcard_test.relux` that uses a wildcard import from the same module. Write a test that uses both `check_running()` and `check_stopped()`.

4. In a third test file, import `check_running as verify_up` and `StartWorker as Worker`. Write a test that uses both under their aliased names.

---

Next: [Condition Markers](15-condition-markers.md) — conditionally skipping or running tests based on environment
