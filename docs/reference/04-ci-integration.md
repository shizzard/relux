# CI Integration

Relux can produce TAP and JUnit output for integration with CI systems:

```text
relux run --tap --junit
```

This writes `results.tap` and `junit.xml` into the run directory at
`relux/out/run-<timestamp>-<id>/`. A `relux/out/latest` symlink always
points to the most recent run. The run directory also contains `index.html`
(summary report), `logs/` (per-test event logs), and `artifacts/`.

**Key point:** Always archive the entire run directory, not just the XML/TAP
files. The JUnit XML references log files via relative paths, and CI systems
that support attachments (Jenkins, GitLab) can link directly to per-test
`event.html` logs when the directory structure is preserved.

Each per-test directory under `logs/` contains two artifacts:

- `events.json` — canonical structured payload (spans, events, buffer events,
  outcome record, embedded source files). Stable schema, consumable by
  external tooling. The top-level `outcome` field is a tagged enum
  (`{ "kind": "pass" }`, `{ "kind": "fail", ... }`,
  `{ "kind": "cancelled", reason: { type: "test-timeout" | "suite-timeout" |
  "fail-fast" | "sigint", ... }, ... }`, `{ "kind": "skip", ... }`)
  that carries the verdict alongside failure-, cancellation-, or skip-specific
  context. Each `Span.location` and `Event.source` carries
  `{ file, line, start, end }` where `start` / `end` are byte offsets into the
  matching entry of the top-level `sources` map (relative path -> file
  contents). Files referenced by no span or event are not embedded.
  The top-level `artifacts` field lists every file the test wrote under its
  `artifacts/` directory: each entry is `{ path, size, mime }` with `path`
  forward-slash and relative to that directory. The list is sorted so files
  precede subdirectory contents at every directory level.
- `event.html` — self-contained Svelte SPA viewer. The structured log,
  highlight.js core, Relux language definition, and the viewer bundle
  are each gzipped, base64-encoded, and inlined into
  `<script type="application/octet-stream">` payload tags. A small
  bootstrap script decompresses them in-browser via
  [`DecompressionStream`](https://developer.mozilla.org/docs/Web/API/DecompressionStream),
  sets `window.RELUX_DATA`, and runs the three JS payloads in order
  (hljs core → Relux grammar → viewer). Opens directly via `file://`;
  no server required. Requires Chrome 80+ / Firefox 113+ / Safari 16.4+;
  older browsers see a one-line message and nothing else. This is the
  recommended human entry point and the link target used by the
  run-summary `index.html`, JUnit `[[ATTACHMENT|...]]` markers, and
  TAP `log:` fields.

### Skipped-test logs

Tests skipped by a marker — either `# skip if X` evaluating true or
`# run if X` evaluating false, on the test itself or on any effect/function
it depends on — produce a per-test log alongside passed and failed tests.
The skipped-test log contains only the MARKERS section: the synthetic
`markers` span tree with one `marker-eval` child per evaluated marker
(including any flaky markers that ran before the skip-causing one).
Opening `event.html` focuses the marker that triggered the skip and expands
its ancestors so the tree is unfolded. For a skip propagated from a fn or
effect, the focused marker is the originating one on that fn/effect, not
on the test.

Tests skipped for other reasons (e.g., "skipped because an earlier test
caused fail-fast and this test was never started") do not produce a log:
there are no marker evaluations to show; the actionable information lives
on the test that caused the cancellation.

### Cancelled outcome

A test that *was* started but did not run to completion produces a
`Cancelled` outcome — distinct from `Fail`. Sources:

- **Test timeout (`~T` on the test)**: the per-test watchdog fired.
  Carried as `reason: { type: "test-timeout", duration_ms }`.
- **Suite timeout**: the suite-wide watchdog fired. Other live tests are
  cancelled with `reason: { type: "suite-timeout", duration_ms }`.
- **Fail-fast**: a sibling test failed with `--strategy fail-fast`. Live
  tests are cancelled with `reason: { type: "fail-fast", trigger_test }`.
- **SIGINT**: the CLI process received SIGINT. Live tests are cancelled
  with `reason: { type: "sigint" }`.

Cancelled outcomes:

- Exit nonzero from `relux run` (same as failures).
- Render as `not ok` in TAP, with a diagnostic block carrying
  `cancellation: <reason-tag>`.
- Render as `<error type="cancelled" message="cancelled: <reason-tag>"/>`
  in JUnit XML (distinct from `<failure>` and `<skipped>`).
- Render as a `cancelled` row in the HTML run index and a `CANCELLED`
  pill in the per-test viewer.
- A `cancelled` event in `events.json` marks the exact point where the
  VM observed the cancel, on the span execution was inside at that
  moment.

Flaky-retry semantics: a test marked `# flaky` is retried on `Fail` *and*
on `Cancelled { reason: TestTimeout }` (the test's own clock running out —
exactly what scaled-timeout retries target). Other cancellation reasons
(suite-timeout, fail-fast, SIGINT) are *not* retried.

### Artifacts

Anything a test writes under `$__RELUX_TEST_ARTIFACTS` is enumerated in
`events.json` under `artifacts` and surfaced in the viewer through an
`artifacts` modal (AppBar chip, hotkey `A`). Each entry is a relative link
that opens in a new browser tab; this works whether `event.html` is opened
directly via `file://` or served over HTTP. The chip is rendered as
disabled when the test wrote no artifacts.

The viewer bundle is committed at `vendor/relux-viewer.js.gz`; regenerate it
(and the TypeScript schema bindings) with `just viewer-build`.

---

## GitLab CI

GitLab natively consumes JUnit XML via `artifacts:reports:junit`. Archive the
full run directory so that `[[ATTACHMENT|...]]` markers in `<system-out>`
resolve to the event logs.

```yaml
test:
  stage: test
  script:
    - relux run --junit
  artifacts:
    when: always
    paths:
      - relux/out/latest/
    reports:
      junit: relux/out/latest/junit.xml
```

Setting `when: always` ensures artifacts are uploaded even when tests fail.

---

## GitHub Actions

GitHub Actions does not have built-in JUnit support. Use
`actions/upload-artifact` to preserve the run directory, and a third-party
action to surface test results in the PR.

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Run tests
        run: relux run --junit

      - name: Upload test results
        if: always()
        uses: actions/upload-artifact@v7
        with:
          name: relux-results
          path: relux/out/latest/

      - name: Publish test report
        if: always()
        uses: mikepenz/action-junit-report@v5
        with:
          report_paths: relux/out/latest/junit.xml
```

Other JUnit report actions (e.g., `dorny/test-reporter`) work the same way --
point them at `relux/out/latest/junit.xml`.

---

## Jenkins

Use the **JUnit** post-build step to parse results. Install the **JUnit
Attachments Plugin** to make per-test event logs clickable -- it reads the
`[[ATTACHMENT|...]]` markers embedded in `<system-out>`.

```groovy
pipeline {
    agent any
    stages {
        stage('Test') {
            steps {
                sh 'relux run --junit'
            }
            post {
                always {
                    junit testResults: 'relux/out/latest/junit.xml',
                          allowEmptyResults: true
                    archiveArtifacts artifacts: 'relux/out/latest/**',
                                     allowEmptyArchive: true
                }
            }
        }
    }
}
```

With the JUnit Attachments Plugin installed, each test case in the Jenkins UI
will link to its `event.html` log automatically.

---

## Azure DevOps

Use the `PublishTestResults` task to ingest JUnit XML.

```yaml
steps:
  - script: relux run --junit
    displayName: Run tests

  - task: PublishTestResults@2
    condition: always()
    inputs:
      testResultsFormat: JUnit
      testResultsFiles: relux/out/latest/junit.xml
      mergeTestResults: true
      testRunTitle: Relux

  - task: PublishBuildArtifacts@1
    condition: always()
    inputs:
      pathToPublish: relux/out/latest
      artifactName: relux-results
```

---

## Gitea Actions

Gitea Actions uses the same workflow syntax as GitHub Actions. Gitea does not
render JUnit reports natively, but you can archive results and use compatible
actions from the Gitea marketplace.

```yaml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Run tests
        run: relux run --junit --tap

      - name: Upload test results
        if: always()
        uses: actions/upload-artifact@v7
        with:
          name: relux-results
          path: relux/out/latest/
```

The uploaded artifact preserves the full run directory including `index.html`,
which serves as a self-contained test report you can browse locally.

---

## TAP Consumers

The `--tap` flag produces TAP version 14 output in `results.tap`. This is
useful with any TAP consumer (e.g., `tap-diff`, `tap-dot`, Jenkins TAP
Plugin):

```text
# Stream TAP to a formatter
cat relux/out/latest/results.tap | tap-diff

# Or use the file directly with CI plugins that accept TAP input
```

TAP output includes log file paths in YAML diagnostics blocks (`log:` field),
but most CI systems do not parse TAP diagnostics for attachments. Use `--junit`
when you need CI-native log attachment support.
