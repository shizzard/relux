# Built-in Functions

Relux provides built-in functions (BIFs) that are always available without imports. BIFs are divided into two categories based on their **purity** — whether they require a shell context to operate.

## Purity

- **Pure** BIFs do not interact with any shell. They can be called from pure functions, condition markers, overlay expressions, and regular shell blocks.
- **Impure** BIFs require a shell context (they send input or match output). They can only be called inside shell blocks and regular (non-pure) functions.

"Pure" here means shell-independent, not side-effect-free — pure BIFs may still perform I/O (e.g. `sleep`, `log`, `which`).

Shell-independent also does not mean infallible. A `pure fn` body may contain a [pure match](08-pure-matching.md) (`<expr> = <pattern>` / `<expr> ? <pattern>`), and a non-matching pure match is an assertion failure: a `pure fn` that runs one can fail the test through whichever runtime site called it (a `let`, an overlay value, or a shell-block call). The one exception is a marker condition, where a pure-eval failure makes the condition falsy rather than failing the test.

## Pure BIFs

### String

| Function  | Signature              | Returns | Description                                                                                                                                        |
|-----------|------------------------|---------|----------------------------------------------------------------------------------------------------------------------------------------------------|
| `trim`    | `trim(s)`              | string  | Remove leading and trailing whitespace from `s`.                                                                                                   |
| `upper`   | `upper(s)`             | string  | Convert `s` to uppercase.                                                                                                                          |
| `lower`   | `lower(s)`             | string  | Convert `s` to lowercase.                                                                                                                          |
| `replace` | `replace(s, from, to)` | string  | Replace all occurrences of `from` with `to` in `s`.                                                                                                |
| `split`   | `split(s, sep, index)` | string  | Split `s` by `sep` and return the part at `index` (0-based). Returns `""` if the index is out of bounds. Errors if `index` is not a valid integer. |
| `len`     | `len(s)`               | string  | Return the byte length of `s` as a decimal string.                                                                                                 |
| `default` | `default(a, b)`        | string  | Return `a` if it is non-empty, otherwise return `b`.                                                                                               |

### Generators

| Function | Signature       | Returns | Description                                                                                                                                                                               |
|----------|-----------------|---------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `uuid`   | `uuid()`        | string  | Generate a random UUID v4 (e.g. `"550e8400-e29b-41d4-a716-446655440000"`).                                                                                                                |
| `rand`   | `rand(n)`       | string  | Generate a random alphanumeric string of length `n`. Errors if `n` is not a valid integer.                                                                                                |
| `rand`   | `rand(n, mode)` | string  | Generate a random string of length `n` using the given charset `mode`. Modes: `alpha`, `num`, `alphanum`, `hex`, `oct`, `bin`. Errors if `mode` is unknown or `n` is not a valid integer. |

### System

| Function         | Signature          | Returns | Description                                                                                                                                                                                                                                                              |
|------------------|--------------------|---------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `available_port` | `available_port()` | string  | Bind to an ephemeral TCP port on `127.0.0.1` and return the port number. The port is released after the call, so it may be reused — call this close to where the port is needed.                                                                                         |
| `which`          | `which(name)`      | string  | Search `PATH` for an executable named `name`. Returns the absolute path to the first match, or `""` if not found. Checks that the file has an executable permission bit set. If `name` contains a path separator, checks that path directly instead of searching `PATH`. |
| `sleep`          | `sleep(duration)`  | `""`    | Pause execution for `duration`. Accepts [humantime](https://docs.rs/humantime) format: `500ms`, `2s`, `1m30s`, etc. Errors if the duration is invalid.                                                                                                                   |

### Logging

| Function   | Signature        | Returns | Description                                                                       |
|------------|------------------|---------|-----------------------------------------------------------------------------------|
| `log`      | `log(message)`   | string  | Emit `message` to the event log and HTML report. Returns `message`.               |
| `annotate` | `annotate(text)` | string  | Emit `text` as a progress annotation. Renders inline on the live progress line (between the surrounding fn-call `(` and `)`) and is recorded as an event in the structured log. Returns `text`. |

### Hashing

| Function   | Signature     | Returns | Description                                                                                                                                                                                                                                          |
|------------|---------------|---------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `mnemonic` | `mnemonic(s)` | string  | Derive a stable, human-readable id from `s`, formatted `adjective-noun-NNNN` (e.g. `"brave-otter-0042"`). Deterministic across runs and across relux versions. About 2^29 distinct values -- a readable label, not collision-proof and not for security. |
| `sha1`     | `sha1(s)`     | string  | SHA-1 digest of `s` as 40-character lowercase hexadecimal.                                                                                                                                                                                          |

### Time

| Function    | Signature       | Returns | Description |
|-------------|-----------------|---------|-------------|
| `timestamp` | `timestamp(fmt)` | string  | Current UTC time formatted with a GNU `date`-style strftime string. Fractional seconds accept any width (`%<N>f`, `%.<N>f`); an unknown specifier is emitted verbatim. |

`timestamp` always renders the current instant in UTC -- there is no local-timezone mode. It deviates from chrono's strftime in two ways: fractional-second specifiers accept any width, not just chrono's fixed 3/6/9 (`%1f`..`%9f`, `%.1f`..`%.9f`, and widths above 9 right-pad with zeros), and an unknown specifier is emitted verbatim instead of being blanked out. Like `uuid` and `rand`, `timestamp` is non-deterministic across calls -- it reads the system clock -- but it is still a pure BIF because it never touches a shell.

```text
timestamp("%Y-%m-%dT%H:%M:%SZ")  -> 2026-07-28T15:30:45Z
timestamp("%Y%m%d-%H%M%S")       -> 20260728-153045
timestamp("%s")                  -> 1753716645
timestamp("%H%M%S-%6f")          -> 153045-123456
```

## Impure BIFs

### Shell matching

| Function          | Signature               | Returns | Description                                                                                                                                                                     |
|-------------------|-------------------------|---------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `match_prompt`    | `match_prompt()`        | string  | Match the shell prompt configured in `Relux.toml`. Advances the output cursor past the prompt.                                                                                  |
| `match_ok`        | `match_ok()`            | string  | Match the shell prompt, send `echo $?`, match `0`, and match the prompt again. Verifies the previous command exited with status 0.                                              |
| `match_not_ok`    | `match_not_ok()`        | string  | Match the shell prompt, verify the previous command exited with a non-zero status, and match the prompt again. The inverse of `match_ok()`.                                     |
| `match_not_ok`    | `match_not_ok(code)`    | string  | Match the shell prompt, verify the previous command exited with a specific non-zero status `code`, and match the prompt again. Like `match_exit_code(code)` but also asserts the code is non-zero. |
| `match_exit_code` | `match_exit_code(code)` | string  | Send `echo $?`, match `code`, and match the prompt. Verifies the previous command exited with the given status. `code` is passed as a bare literal (e.g. `match_exit_code(1)`). |

### Control characters

| Function         | Signature          | Returns | Description                                             |
|------------------|--------------------|---------|---------------------------------------------------------|
| `ctrl_c`         | `ctrl_c()`         | `""`    | Send `ETX` (0x03) — interrupt the current process.      |
| `ctrl_d`         | `ctrl_d()`         | `""`    | Send `EOT` (0x04) — signal end of input.                |
| `ctrl_z`         | `ctrl_z()`         | `""`    | Send `SUB` (0x1A) — suspend the current process.        |
| `ctrl_l`         | `ctrl_l()`         | `""`    | Send `FF` (0x0C) — clear the terminal screen.           |
| `ctrl_backslash` | `ctrl_backslash()` | `""`    | Send `FS` (0x1C) — send SIGQUIT to the current process. |
