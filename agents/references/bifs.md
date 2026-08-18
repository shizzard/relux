# Built-in Functions

Always in scope; no imports. Split by purity: pure BIFs may be called anywhere; impure BIFs require a shell context. May be shadowed by user-defined function.

## Pure BIFs

### String

| Function | Signature | Returns | Behavior |
|---|---|---|---|
| `trim` | `trim(s)` | string | Strip leading/trailing whitespace. |
| `upper` | `upper(s)` | string | Uppercase. |
| `lower` | `lower(s)` | string | Lowercase. |
| `replace` | `replace(s, from, to)` | string | Replace all occurrences. |
| `split` | `split(s, sep, index)` | string | Split by `sep`; return 0-based `index` (or `""` if out of bounds). |
| `len` | `len(s)` | string | Byte length, as a decimal string. |
| `default` | `default(a, b)` | string | `a` if non-empty, else `b`. |

### Generators

| Function | Signature | Returns | Behavior |
|---|---|---|---|
| `uuid` | `uuid()` | string | Random UUID v4. |
| `rand` | `rand(n)` | string | Random alphanumeric string of length `n`. |
| `rand` | `rand(n, mode)` | string | `mode` in `alpha` / `num` / `alphanum` / `hex` / `oct` / `bin`. |

### System

| Function | Signature | Returns | Behavior |
|---|---|---|---|
| `available_port` | `available_port()` | string | Allocate a probed TCP port on 127.0.0.1 from outside the OS ephemeral range; owned by the running test, freed after its cleanup. -1 on exhaustion. |
| `which` | `which(name)` | string | Absolute path of executable in PATH, or `""` if not found. With a path separator, checks that path directly. |
| `sleep` | `sleep(duration)` | `""` | Pause. Humantime: `500ms`, `2s`, `1m30s`. |

### Logging

| Function | Signature | Returns | Behavior |
|---|---|---|---|
| `log` | `log(message)` | message | Emit to structured event log. |
| `annotate` | `annotate(text)` | text | Render inline on progress line and as a structured event. |

## Impure BIFs

### Shell matching

| Function | Signature | Returns | Behavior |
|---|---|---|---|
| `match_prompt` | `match_prompt()` | string | Match the configured shell prompt; advance cursor. |
| `match_ok` | `match_ok()` | string | Match prompt, send `echo $?`, expect `0`, match prompt. |
| `match_not_ok` | `match_not_ok()` | string | Match prompt, expect non-zero exit, match prompt. |
| `match_not_ok` | `match_not_ok(code)` | string | Like `match_exit_code(code)` and asserts non-zero. |
| `match_exit_code` | `match_exit_code(code)` | string | Send `echo $?`, expect `code`, match prompt. `code` is bare. |

### Control characters

| Function | Bytes | Use |
|---|---|---|
| `ctrl_c()` | `0x03` (ETX) | Interrupt foreground process. |
| `ctrl_d()` | `0x04` (EOT) | Signal end of input. |
| `ctrl_z()` | `0x1A` (SUB) | Suspend foreground process. |
| `ctrl_l()` | `0x0C` (FF) | Clear terminal screen. |
| `ctrl_backslash()` | `0x1C` (FS) | Send SIGQUIT to foreground process. |

## Where each is allowed

- Pure BIFs: pure functions, marker conditions, overlay expressions, `let` RHS, shell-block expressions.
- Impure BIFs: shell blocks, regular (non-pure) functions, effect setup shells, cleanup.

## Pitfalls and best practices

### Use `default` for absent variables instead of bare interpolation

`${VAR}` on an unset name silently becomes `""`. `default(VAR, "...")` gives a deliberate fallback. Bare form in BIF/function-argument position; interpolation form inside a string.

Don't:

```relux
// silent empty string when HOST unset
let host := "${HOST}"
let url  := "http://${HOST}:8080/api"
```

Do:

```relux
// bare in BIF arg
let host := default(HOST, "localhost")
// interpolation inside the string
let url  := "http://${host}:8080/api"
```

### `match_ok` is the canonical "command succeeded" check

Use it (or `match_exit_code(0)`) instead of hand-rolling per-shell success patterns. It's prompt-aware and won't drift if your prompt changes.

Don't:

```relux
> my-cmd
<? exited normally
```

Do:

```relux
> my-cmd
match_ok()
```

### Ports from `available_port` are owned by the running test

Each call returns a distinct port from outside the OS ephemeral range,
probed at allocation. The port stays reserved (within this relux process)
until the allocating test's cleanup completes, so calls in effect config
evaluated long before the service binds are safe -- no call-site
positioning is needed. The `[available_ports]` manifest section narrows
the window; see [project-layout](project-layout.md) > *Sections* for the fields.

## See also

- [interpolation](interpolation.md) -- read if you need to use BIF results in patterns and sends
- [functions](functions.md) -- read if you need to learn about purity context rules
- [markers](markers.md) -- read if you need to use pure BIFs in marker conditions
