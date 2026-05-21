#!/usr/bin/env bash
# Copy the canonical hljs grammar, theme CSS, and the vendored
# highlight.js v11 into each book directory. The vendored hljs goes
# into `theme/highlight.js` - mdBook's theme-override mechanism
# replaces its built-in v10.1.1 with our v11 so the runtime report and
# the books use the same hljs version (and grammar). The per-book
# copies are all gitignored; this step regenerates them before mdbook
# reads them.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

hljs_gz=vendor/highlight-11.11.1.min.js.gz
grammar=crates/relux-runtime/src/report/highlight-relux.js
css=docs/_theme/relux.css
for book in docs/dsl-tutorial docs/reference docs/suite-tutorial; do
    mkdir -p "$book/theme"
    gunzip -c "$hljs_gz" > "$book/theme/highlight.js"
    cp "$grammar" "$book/highlight-relux.js"
    cp "$css" "$book/relux.css"
done
