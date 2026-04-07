# Changelog

## [0.3.1](https://github.com/shizzard/relux/compare/v0.3.0...v0.3.1) (2026-04-07)


### Bug Fixes

* **ci:** fix gh-pages initialization for first deployment ([2760e6f](https://github.com/shizzard/relux/commit/2760e6f8e8f20957d575cf81531bdbbf7ca5a177))
* **docs:** remove duplicate reference link in README ([cc7146f](https://github.com/shizzard/relux/commit/cc7146f6f60ad444462741a7870dd0476dc33058))
* **release:** remove component prefix from release tags ([87490b9](https://github.com/shizzard/relux/commit/87490b9d22c5fab1f3015df48d103ff4721ef9ca))

## [0.3.0](https://github.com/shizzard/relux/compare/relux-v0.2.0...relux-v0.3.0) (2026-04-07)


### ⚠ BREAKING CHANGES

* **effects:** effect syntax changed from `effect Name -> shell` to `effect Name { expect, expose, start, shell, cleanup }`. The `need` keyword is replaced by `start`. Qualified shell blocks require the `shell` keyword.
* **runtime:** __RELUX_RUN_ARTIFACTS renamed to __RELUX_TEST_ARTIFACTS and now points to a per-test directory instead of a shared run directory.
* **runtime:** RunReport now requires wall_duration and jobs fields. execute() returns ExecuteResult instead of Vec<TestResult>.
* **runtime:** test and effect bodies now require `let` declarations before `need` declarations. The previous order (needs before lets) will no longer parse.
* **runtime:** EffectManager::new() and Vm::new() signatures changed. EventCollector removed in favor of EventSink.
* **runtime:** runtime module restructured — vm, effect, observe, and report are now separate submodules. Failure enum gains Cancelled variant.
* **dsl:** resolver public API changed — resolve() now takes a SourceLoader and returns a Suite with NewPlan variants (Runnable, Skipped, Invalid). Old ir.rs, plan.rs, scope.rs, effect_graph.rs removed. Runtime cmd_run stubbed pending adaptation.
* **dsl:** comment syntax changes from `#` to `//` and condition markers change from `[...]` to `# ...` in all .relux files.
* **runtime:** negative match operators <!?, <!=, <~dur!?, <~dur!= are removed. Use fail patterns (!?, !=) instead.
* **cli:** resolve() public API changed from resolve(roots, project_root, lib_dir) to resolve(project_root, paths).
* **dsl:** condition marker syntax now requires quoted strings with interpolation ("${CI}") instead of bare identifiers (CI).
* **parser:** effect head syntax changed from `effect Name -> shell name` to `effect Name -> name`.

### Features

* add built-in functions and bare ident/number syntax ([1b70059](https://github.com/shizzard/relux/commit/1b700595371016f0761a5b525d75846418ce62ee))
* add condition markers with pre-declaration placement ([b0a7651](https://github.com/shizzard/relux/commit/b0a7651fbd906413983b63594a0d42930c89b0d7))
* add IntelliJ IDEA plugin with syntax highlighting ([d6c77eb](https://github.com/shizzard/relux/commit/d6c77eb6b7dae70e278b7a4d0b0c0c37b46eaf52))
* add negative match operators and inline timeout overrides ([d694732](https://github.com/shizzard/relux/commit/d69473272ed77a12ef813ec3ff6ff6884d8db120))
* **bifs:** add available_port() built-in function ([80ab39b](https://github.com/shizzard/relux/commit/80ab39b40662eecfcc14f6f24ca3bfd0ed41b937))
* **bifs:** add default/2 pure built-in function ([e67351a](https://github.com/shizzard/relux/commit/e67351a466043664216152a7e77e54e784ac2d01))
* **bifs:** add match_not_ok(exit_code) arity-1 variant ([8a84296](https://github.com/shizzard/relux/commit/8a84296e5e4ccfc174a0e4099fe3b185f928ccc2))
* **bifs:** add which, match_not_ok and sentinel-wrap exit code checks ([ca6388c](https://github.com/shizzard/relux/commit/ca6388c888edaf5c56e6a027e5a26cf41513ff10))
* **cli:** add --manifest flag and move file discovery into resolver ([ae65d20](https://github.com/shizzard/relux/commit/ae65d20d8b2b07d49880e80fcefb9633eb4797ea))
* **cli:** add run history and --rerun support ([b4c40c5](https://github.com/shizzard/relux/commit/b4c40c5fcb565dae67b4449bc6f353485ec9bb2c))
* **dsl:** add inline per-test timeout syntax and rename case to test ([f5f9d78](https://github.com/shizzard/relux/commit/f5f9d78e01c4411d4989fd12aeee4404f39b4e5e))
* **dsl:** add pure functions ([a5fae3b](https://github.com/shizzard/relux/commit/a5fae3be0fb2c9c1c4b8d6c0966d94dfc7cb131f))
* **dsl:** implement R003 lexer/parser rework ([11577f8](https://github.com/shizzard/relux/commit/11577f897382f0ed2f55f7141a1211d4740b7ea0))
* **dsl:** implement R004 resolver rework ([276a30a](https://github.com/shizzard/relux/commit/276a30a899f6799d07e4ea74b9b9ffb578e840ff))
* **dsl:** improve diagnostics with ariadne annotations and relative paths ([0596238](https://github.com/shizzard/relux/commit/05962387847de6efe9a8d51ae35eb2b55f7e88f3))
* **dsl:** make condition marker modifier optional ([208dedc](https://github.com/shizzard/relux/commit/208dedc2acfb728cc9f3cc9f3e092ccee3697caa))
* **dsl:** replace flat MarkerData with expression-based conditions ([a27fd22](https://github.com/shizzard/relux/commit/a27fd22ddf783075662156cc7f0f9198981ed98b))
* e2e self-test suite ([52530ef](https://github.com/shizzard/relux/commit/52530ef729631ff2c68e89e0f5fbfd7948ffd9db))
* **editor:** add VS Code/Cursor syntax highlighting extension ([86cf69f](https://github.com/shizzard/relux/commit/86cf69fa81b884167c4bb3d480a1211c94209152))
* **effects:** rework effect system with expect, expose, start, and cleanup ([21f12aa](https://github.com/shizzard/relux/commit/21f12aa44318759db5e4812d4ec38ec045093965))
* **history:** add run history analysis with unified data pipeline ([e614c01](https://github.com/shizzard/relux/commit/e614c0133044449192d3cdfa197567d4ce99f825))
* initial implementation of the relux test framework ([340a948](https://github.com/shizzard/relux/commit/340a948c41e4bdcadd5a1fdd6b17f9d627b2bc69))
* **lang:** add assertion timeouts with @ prefix ([2d8fb0c](https://github.com/shizzard/relux/commit/2d8fb0c2b861d9deed83622c12f930eb17723ebe))
* log() BIF emits to rich HTML logs, improve test scaffold ([249693a](https://github.com/shizzard/relux/commit/249693adc611a9aa4f99a0d1fe80d2c841dddc71))
* **logging:** add per-shell plaintext and rich HTML event logs ([d3e0530](https://github.com/shizzard/relux/commit/d3e0530548654ed9d21cbaeb6d66d06e328f7622))
* **parser:** drop redundant `shell` keyword from effect head ([db6e2b2](https://github.com/shizzard/relux/commit/db6e2b21d5508f4c302c834e47f956df4677ea1b))
* **reporter:** add buffer visualization column to HTML test logs ([269454c](https://github.com/shizzard/relux/commit/269454c3f55d4fa0f0813a4754008eef56ca7b1d))
* **reporter:** redesign test run output to match cargo test style ([7b402d0](https://github.com/shizzard/relux/commit/7b402d08b70ec5280cca4dc43df10fc147260061))
* **reporting:** add TAP14 and JUnit XML output formats ([ba93eb0](https://github.com/shizzard/relux/commit/ba93eb0c1c9dfbf716073c83fd992c714a11ff5c))
* **resolver:** require explicit `as` alias for effect shell access ([6f55131](https://github.com/shizzard/relux/commit/6f55131f82ee50c54c2bd4b6a8453056149765b2))
* **runtime:** add buffer reset via bare match operator ([3b1d51a](https://github.com/shizzard/relux/commit/3b1d51a2cdd5d88b7348fda88f8521f3e062630f))
* **runtime:** add control character BIFs and dynamic shell prompt ([cf34a84](https://github.com/shizzard/relux/commit/cf34a8477b6c1f188eac24098c057e586e11a7ae))
* **runtime:** add parallel test execution with TUI progress ([1ce4b4b](https://github.com/shizzard/relux/commit/1ce4b4bb98eacc9857199a3277432f13d47e0267))
* **runtime:** add real-time test progress reporting ([5ae0edd](https://github.com/shizzard/relux/commit/5ae0edda31614c6c161c41cdfc2cf11d499c14fb))
* **runtime:** add sync-point BIFs and multiline+CRLF regex matching ([1ca3c41](https://github.com/shizzard/relux/commit/1ca3c41c382065dc304e1626e7c246960eee8bb5))
* **runtime:** add test env vars, nested suite filtering, and fix truncation panic ([c3ac06e](https://github.com/shizzard/relux/commit/c3ac06e092d2c419c6669c660bfa1eff6fbdf4aa))
* **runtime:** allow test-level lets before needs for overlay access ([c573c10](https://github.com/shizzard/relux/commit/c573c10ae5345147edfd9db8783dc42bc0699c8f))
* **runtime:** clear fail pattern with bare !? or != operator ([d9d56cb](https://github.com/shizzard/relux/commit/d9d56cb45ac7cd9b5325f783c077c29186e04b86))
* **runtime:** enrich HTML event logs with structured data and improved presentation ([a0a4c04](https://github.com/shizzard/relux/commit/a0a4c04e324ee366039391c237a73644d1df88ad))
* **runtime:** implement flaky marker retry semantics ([afa93dd](https://github.com/shizzard/relux/commit/afa93ddfa6f89ba4263c433bdfb58f61b00c4c8a))
* **runtime:** implement R005 runtime rework ([ff86721](https://github.com/shizzard/relux/commit/ff867214c0b4a34938dc43293e5b123504faa5ec))
* **runtime:** per-test artifacts dir and misc improvements ([dead041](https://github.com/shizzard/relux/commit/dead041af3c9dd4deeedfc4caf278036f95aa6e0))
* **runtime:** print failure diagnostics inline with test results ([298271e](https://github.com/shizzard/relux/commit/298271e8edecc34a68505faa3ac401805e53b121))
* unified relux binary with Relux.toml configuration ([d6995dd](https://github.com/shizzard/relux/commit/d6995dd17377439e030a9fcd3a4a7ebab3091006))


### Bug Fixes

* **cli:** color skipped test output yellow ([4c03548](https://github.com/shizzard/relux/commit/4c0354862d11d1eeb5284ef37253590fc377b413))
* **cli:** report clear error for non-existent test file paths ([8485fa0](https://github.com/shizzard/relux/commit/8485fa0ecb1d27518a22700e238c9ecd07ee45e1))
* condition marker grammar leaking past closing bracket ([d99240b](https://github.com/shizzard/relux/commit/d99240b4e3d5bdd42aceffa63f3545a10bccfca8))
* **diagnostics:** correct misleading label on condition marker skip ([7a30d12](https://github.com/shizzard/relux/commit/7a30d121b75f946dd01b34f41dda8077c3eaacd4))
* **intellij:** accept any letter sequence in duration units ([2df8454](https://github.com/shizzard/relux/commit/2df84544ec1c404a314e5e69c6c99801e81218a9))
* **intellij:** add numeric literal support to lexer ([8dd143c](https://github.com/shizzard/relux/commit/8dd143c864e9b2b7d7735ddf07730cd921c7587c))
* **lexer:** require braces for numeric interpolation in payloads and strings ([fb70f5f](https://github.com/shizzard/relux/commit/fb70f5f1fd202e8b64951d5a2e7926ce3872c818))
* resolve imports from lib directory search path ([7351599](https://github.com/shizzard/relux/commit/73515999788f92da6cad9bf4dd9f4e725fe33b3c))
* **resolver:** resolve cross-module function calls from correct scope ([8470d62](https://github.com/shizzard/relux/commit/8470d62383ef3d24b8205383d0713f43f7bd5f42))
* **resolver:** uniform purity diagnostics for all impure-in-pure violations ([00a4100](https://github.com/shizzard/relux/commit/00a41000524de1d41fc18402f367e25ba13f7d24))
* **runtime:** guard against regex matching partial lines in buffer ([9a088e0](https://github.com/shizzard/relux/commit/9a088e00cd78736e55af0ee7ce040bc8f9096440))
* **runtime:** isolate function scope from caller's variables ([179644a](https://github.com/shizzard/relux/commit/179644a87b23da5c7f5e90eeea8f99ee79539615))
* **runtime:** replace background fail watcher with inline checking ([0b679b5](https://github.com/shizzard/relux/commit/0b679b55f2959f8d3f61dfe6bc577a0eab36a38c))
* **runtime:** resolve overlay variables in sub-need and effect-let expressions ([9c123cd](https://github.com/shizzard/relux/commit/9c123cd65acc10e603cf2a212bb843f3599fe551))
* **runtime:** restore fail pattern after function returns ([722a67c](https://github.com/shizzard/relux/commit/722a67c814a6e8f8b18e5bd6697aee0fd313ff2c))
* **runtime:** restore qualified shell names and emit missing events ([9488810](https://github.com/shizzard/relux/commit/9488810815aa502e408182a906b5dd384e5bce6f))
* **runtime:** track cancellation reason explicitly and make sleep cancellable ([626a08b](https://github.com/shizzard/relux/commit/626a08ba48bc2b40a5cc948e140d034a0cf3422c))
* **vm:** free output buffer after each match ([875efc4](https://github.com/shizzard/relux/commit/875efc462be8bb120c50771847688d076f3fffee))


### Code Refactoring

* **runtime:** unify event system with EventSink and RuntimeContext ([742de52](https://github.com/shizzard/relux/commit/742de52f8c0b58683afcbbb597412cccc574f32f))
