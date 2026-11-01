<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: galion.app legal pages, DRAFT (swarm/galion-legal-pages-20261101)

Run: 2026-11-01 15:20 +01:00 (Europe/Zagreb). Local only, port 19986, nothing deployed, nothing pushed.

- Pages: `crates/galion-one/home/{privacy,terms,refunds}.html`, compiled in by `src/home.rs`, served at
  `/privacy`, `/terms`, `/refunds` (and with a trailing slash) on the home hosts only (`ONE_HOME_HOSTS`,
  default `galion.app,www.galion.app`). Markdown sources: `docs/legal/*.md`. All marked
  "Draft, not legal advice", `noindex`, placeholders in `[brackets]`; no company details invented.
- Home page footer now links Privacy, Terms, Refunds (`index.html.gz` regenerated with `gzip -9 -n -k -f`).
- `cargo test -p galion-one`: 105 passed, 1 ignored (new: `home::tests::legal_pages_are_marked_draft_and_self_contained`,
  `home::tests::legal_routes`); `cargo fmt --check` clean; `cargo clippy -p galion-one --all-targets -- -D warnings` clean.
- curl, Host: galion.app: `/privacy` 200 17,191 B, `/terms` 200 14,390 B, `/refunds` 200 13,009 B, same CSP and
  nosniff headers as `/`, body byte-identical to the file. Host one.galion.app and localhost: `/privacy` 404 (falls through).
- Screenshots (chrome-headless-shell, 390x844 @2x and 1440x900, plus full-length): `/workspace/swarm/tmp/legal/shots/`.
  Jost is not on the box (`/font/jost.woff2` is empty locally), so headings use the fallback font, like the home page test.
