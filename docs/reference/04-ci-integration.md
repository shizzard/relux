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
  failure record). Stable schema, consumable by external tooling.
- `event.html` — self-contained Svelte SPA viewer with the JSON inlined as
  `window.RELUX_DATA` and the gzipped bundle decompressed into a `<script>`
  tag. Opens directly via `file://`; no server required. This is the
  recommended human entry point and the link target used by the run-summary
  `index.html`, JUnit `[[ATTACHMENT|...]]` markers, and TAP `log:` fields.

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
