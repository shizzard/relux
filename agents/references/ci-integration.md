# CI Integration

Output formats, scaling for slower environments, and what to archive.

## Output formats

- `--junit` -- writes `junit.xml` into the run directory. Each test case carries `[[ATTACHMENT|...]]` markers in `<system-out>` that point at the per-test `event.html`.
- `--tap` -- writes `results.tap` (TAP v14) into the run directory. Per-test `log:` and `log_json:` YAML fields point at `event.html` / `events.json`.
- Both flags can be passed together.

## Run directory layout

```text
relux/out/run-<timestamp>-<id>/
|-- index.html              # browsable run summary
|-- junit.xml               # if --junit
|-- results.tap             # if --tap
|-- run_summary.toml
|-- artifacts/              # run-level artifacts directory (CLI-managed)
`-- logs/<test>/
    |-- event.html          # self-contained viewer
    |-- events.json         # canonical structured payload
    `-- artifacts/          # per-test artifacts (where $__RELUX_TEST_ARTIFACTS points)
```

`relux/out/latest` is a symlink to the most recent run.

## Outcome -> CI exit

- `pass` -- nothing.
- `fail` -- non-zero exit.
- `cancelled` -- non-zero exit. JUnit `<error type="cancelled" ...>`; TAP `not ok` with `cancellation:` diagnostic.
- `skip` -- does not affect exit. JUnit `<skipped>`; TAP `not ok # SKIP`.

Cancellation reasons (`test-timeout`, `suite-timeout`, `fail-fast`, `sigint`) are visible in the per-test `events.json` and in JUnit/TAP diagnostics.

## Scaling for slow hardware

- `-m`, `--timeout-multiplier <N>` scales all tolerance (`~`) timeouts. Assertion (`@`) timeouts are unscaled by design.
- Default `1.0`. Common CI values: `2.0` or `3.0`.

## Flaky retries

- `--flaky-retries <N>` overrides `[flaky].max_retries`.
- `--flaky-multiplier <N>` overrides `[flaky].timeout_multiplier`.
- Retried on `Fail` and on `Cancelled { reason: test-timeout }` only. Suite-timeout, fail-fast, and SIGINT cancellations are not retried.

## Pitfalls and best practices

### Archive the whole run directory, not just `junit.xml`

The XML references log files via relative paths. CI plugins that resolve `[[ATTACHMENT|...]]` need the surrounding `logs/` tree to make `event.html` clickable.

Don't:

```yaml
artifacts:
  paths:
    - relux/out/latest/junit.xml
```

Do:

```yaml
artifacts:
  when: always
  paths:
    - relux/out/latest/
```

### Multiply timeouts in CI, don't rewrite them per test

Editing `~Ns` values to satisfy slow CI scatters environment-specific numbers across the suite. `-m 3` (or `2`) scales the whole suite uniformly.

Don't: edit `<~5s? ready` to `<~30s? ready` in tests.

Do:

```bash
relux run --junit -m 3
```

### Always upload artifacts on failure (`when: always` / `if: always()`)

Default CI behaviour is to skip artifact upload on failure. Force it: the run directory is the only post-mortem surface.

Don't:

```yaml
artifacts:
  paths:
    - relux/out/latest/
```

Do:

```yaml
artifacts:
  when: always
  paths:
    - relux/out/latest/
```

(GitHub Actions: `if: always()` on the upload step.)

## See also

- [cli-reference](cli-reference.md) -- read if you need `--junit`, `--tap`, `-m`, or `--strategy fail-fast` flags
- [timeouts](timeouts.md) -- read if you need `~` vs `@` semantics or the multiplier
- [viewer](viewer.md) -- read if you need to know what CI plugins link to
- [events-failures](events-failures.md) -- read if you need to distinguish fail vs cancelled outcomes
