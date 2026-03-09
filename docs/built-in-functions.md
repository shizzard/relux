# Built-in Functions

Relux provides built-in functions (BIFs) that are always available without imports. BIFs are divided into two categories based on their **purity** — whether they require a shell context to operate.

## Purity

- **Pure** BIFs do not interact with any shell. They can be called from pure functions, condition markers, overlay expressions, and regular shell blocks.
- **Impure** BIFs require a shell context (they send input or match output). They can only be called inside shell blocks and regular (non-pure) functions.

"Pure" here means shell-independent, not side-effect-free — pure BIFs may still perform I/O (e.g. `sleep`, `log`, `which`).

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
| `annotate` | `annotate(text)` | string  | Emit `text` as a progress annotation (visible in verbose output). Returns `text`. |

## Impure BIFs

### Shell matching

| Function          | Signature               | Returns | Description                                                                                                                                                                     |
|-------------------|-------------------------|---------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `match_prompt`    | `match_prompt()`        | string  | Match the shell prompt configured in `Relux.toml`. Advances the output cursor past the prompt.                                                                                  |
| `match_ok`        | `match_ok()`            | string  | Match the shell prompt, send `echo $?`, match `0`, and match the prompt again. Verifies the previous command exited with status 0.                                              |
| `match_not_ok`    | `match_not_ok()`        | string  | Match the shell prompt, verify the previous command exited with a non-zero status, and match the prompt again. The inverse of `match_ok()`.                                     |
| `match_exit_code` | `match_exit_code(code)` | string  | Send `echo $?`, match `code`, and match the prompt. Verifies the previous command exited with the given status. `code` is passed as a bare literal (e.g. `match_exit_code(1)`). |

### Control characters

| Function         | Signature          | Returns | Description                                             |
|------------------|--------------------|---------|---------------------------------------------------------|
| `ctrl_c`         | `ctrl_c()`         | `""`    | Send `ETX` (0x03) — interrupt the current process.      |
| `ctrl_d`         | `ctrl_d()`         | `""`    | Send `EOT` (0x04) — signal end of input.                |
| `ctrl_z`         | `ctrl_z()`         | `""`    | Send `SUB` (0x1A) — suspend the current process.        |
| `ctrl_l`         | `ctrl_l()`         | `""`    | Send `FF` (0x0C) — clear the terminal screen.           |
| `ctrl_backslash` | `ctrl_backslash()` | `""`    | Send `FS` (0x1C) — send SIGQUIT to the current process. |
