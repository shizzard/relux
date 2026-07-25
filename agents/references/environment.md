# Environment

How the process environment is assembled for each test: hierarchical `.env` discovery, layering, precedence, and provenance.

## Layer order

Lowest precedence to highest (later wins):

| Layer | Source | Provenance tag |
|---|---|---|
| Host process env | the environment `relux` was launched with | `base` |
| `.env` files | one per directory, suite root down to the test's dir | `dot-env` (carries `path`) |
| `__RELUX_*` internals | run/test values injected by the runtime | `relux-internal` |

`let` bindings inside a test/shell sit above every env layer (see [interpolation](interpolation.md)). `__RELUX_*` internals cannot be shadowed by a `.env` or host env.

## `.env` discovery

- Discovery walks from the suite root (the `Relux.toml` directory) down to the directory of the test file being run, loading a `.env` from every directory on that path.
- A test sees only the `.env` files on its own path. A test directly under `relux/tests/` never picks up `relux/tests/deep/.env`.
- Deeper files win: a key set in both a shallow and a deep `.env` resolves to the deep value.

## File format

- `KEY=value` per line; `#` starts a comment; blank lines are ignored.
- A value may reference an already-resolved variable via `${VAR}` -- resolved through the layered env (shallower `.env` files and the host env), never through the process env directly.
- A malformed `.env` is a hard error: `relux run`, `relux check`, and `relux dump` all exit non-zero and name the offending file.

## Provenance in the log

- `events.json` records each winning value's origin. Every `env.bootstrap` entry is `{ key, value, source }`, where `source` is a tagged `EnvSourceRecord`: `{ "kind": "base" }`, `{ "kind": "dot-env", "path": "<file>" }`, or `{ "kind": "relux-internal" }`. (`effect-overlay` exists on the type but does not appear in `bootstrap`.)
- The viewer's env modal groups entries by this provenance.
- Full shape: [events-schema](events-schema.md) > *Top-level fields*.

## Pitfalls and best practices

### Commit a `.env`, don't export ad hoc

A committed `.env` makes the suite reproducible: every developer and CI machine sees the same values without exporting anything by hand. Depending on the launching shell's environment makes runs depend on invisible per-machine state.

Don't:

```bash
export DB_URL=postgres://localhost/test
relux run
```

Do:

```ini
# .env, committed beside Relux.toml
DB_URL=postgres://localhost/test
```

### Know which `.env` files a test is on the path for

A `.env` applies only to tests at or below its directory. Putting a value in a deep `.env` and expecting a sibling test to see it fails silently.

Don't:

```text
relux/tests/
|-- a/.env          # sets SHARED
|-- a/a_test.relux
`-- b/b_test.relux  # never sees SHARED
```

Do:

```text
relux/tests/
|-- .env            # sets SHARED for both a/ and b/
|-- a/a_test.relux
`-- b/b_test.relux
```

## See also

- [interpolation](interpolation.md) -- read for `${VAR}` lookup order and shadowing
- [markers](markers.md) -- read for how condition markers read env at evaluation time
- [project-layout](project-layout.md) -- read for the directory convention `.env` files sit in
- [events-schema](events-schema.md) -- read for the `env.bootstrap` provenance shape
