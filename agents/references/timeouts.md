# Timeouts

Match-operation deadlines. Two kinds with different scaling semantics.

## Two kinds

| Kind | Operator | Scaled by `--timeout-multiplier`? | Meaning |
|------|----------|-----------------------------------|---------|
| Tolerance | `~` | Yes | "Be patient up to this long." Absorbs environmental variability. |
| Assertion | `@` | No | "The system must respond within this." Correctness check. |

## Forms

| Form | Meaning |
|---|---|
| `~3s` | set tolerance timeout for the current shell |
| `@2s` | set assertion timeout for the current shell |
| `<~5s? <regex>` | inline tolerance override for one match |
| `<@500ms= <literal>` | inline assertion override for one match |
| `test "name" ~30s { ... }` | test-level tolerance |
| `test "name" @30s { ... }` | test-level assertion |

Inline override for a multimatch block uses the same prefix, with patterns on their own lines:

```relux
<~10s{
    ? ^a$
    ? ^b$
}
```

- Duration format (compact humantime, no spaces): `500ms`, `2s`, `1m30s`.
- Shell-scoped `~`/`@` persists until another timeout operator replaces it.
- Inline override applies to that single operation; does not change the shell's scoped timeout.
- Scoped timeouts revert at function-return: a `~5s` inside `fn` does not leak past the call.

## Configuration defaults

`Relux.toml` `[timeout]` section:

| Key | Default | Scope |
|-----|---------|-------|
| `match` | `5s` | Default for every match op (tolerance). |
| `test` | `5m` | Per-test budget (tolerance). |
| `suite` | `10m` | Whole-run budget (tolerance). |

All three are tolerance values -- they are scaled by `--timeout-multiplier`.

## Flaky multiplier

When a test marked `# flaky` retries, tolerance timeouts scale by `base * m^(retry-1)` where `m` is `[flaky].timeout_multiplier`. Assertion timeouts are never scaled.

## Pitfalls and best practices

### Don't put test-level timeouts on `.relux` fixtures

The suite default covers them. A per-test `@30s` couples the fixture to one environment's timing and breaks under load or on slower hardware.

Don't:

```relux
test "smoke" @30s {
    shell s {
        > true
        <? .
    }
}
```

Do:

```relux
test "smoke" {
    shell s {
        > true
        <? .
    }
}
```

### Use the multiplier in CI, not per-test edits

Bumping `~Ns` values to satisfy a slow CI host scatters environment-specific numbers through the suite. Run with `-m 2` or `-m 3` instead.

Don't: edit the suite to make CI green.

Do:

```bash
relux run --manifest tests/Relux.toml -m 3
```

### Pick `~` vs `@` by intent

`~` says "be patient." `@` says "this is a deadline I'm asserting on the system." If a timeout is part of the test's contract (e.g. SLO), use `@`. If it's there to tolerate jitter, use `~`.

Don't:

```relux
> probe
<? ack @2s
```

(unless 2s is genuinely an asserted bound)

Do:

```relux
> probe
<? ack ~2s
```

## See also

- [matching](matching.md) -- read if you need to know where inline overrides apply
- [multimatch](multimatch.md) -- read if you need the `<~Ns{ ... }` form for blocks
- [project-layout](project-layout.md) -- read if you need `[timeout]` configuration defaults
- [ci-integration](ci-integration.md) -- read if you need `--timeout-multiplier` usage in CI
