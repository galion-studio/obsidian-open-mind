<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Desk landing: the whole-project overview (`/console`)

Branch `swarm/landing-overview`, based on `swarm/console-ui-rust` c242fed. Written 2026-10-20 23:05 CEST (+02:00).

The console's main page (`/console`, the Desk; `/` redirects here) is no longer a list of rooms. It is one
screen that shows the whole of PROJECT 42 / Gaia:

1. **Vision**: "Discovering the universe together with humans." and Slavik's top-priority rule (rank 1,
   `[priority_rules]` in `config/swarm/swarm-settings.toml` on swarm/settings-snapshot aab8d85).
2. **At a glance**: 9 functions, 52 systems, 46 crates, 7 sectors, 200 tasks in 47 sessions, 18–20 agents measured at once.
3. **Sector map**: 9 cards (Gaia, Hermes, Console, Voice · Galion One, Nerve + cortex, VPS operations,
   Integration · CI, Security, Docs · white paper), each with a status badge, the facts behind it and the branch + SHA.
4. **The automated process**: inline SVG flow (Slavik → Hermes coordinator → 7 sector planners with
   useful/target agent bars → agents ↔ queue + auto-restart), a capacity chart on one linear scale
   (ceiling 250 with auto-accept vs 18–20 measured, wave 3 at 17, Hermes governor 16), the measurement log, and
   the plan totals table (47 / 200 / 645 / 407; a unit test checks the per-sector rows add up to it).
5. **Waiting for Slavik**: the three items of `docs/SLAVIK-TODO.md` (swarm/slavik-todo 87fdd91): HADES admin key (open),
   OpenRouter credit (done, reported, not re-verified), strict HTTP handling approval (open, swarm/common-migrate-2 123eab2).
6. Footer: "facts as of" time and the four sources.

## Updating the facts
All words and numbers live in **`crates/console-ui/src/overview.rs`** (plain Rust constants, no HTML).
`crates/console-ui/src/pages/overview.rs` only lays them out and draws the SVG from the numbers, so editing a
number moves the bars. Compile-time asserts and unit tests keep the numbers coherent (plan rows sum to the
totals, Gaia function counts sum to 52, measured ≤ ceiling, governor ≤ measured). The console card's
"N of 31 routes in Rust" line is computed from `console_ui::PORTED`.

Chosen over a TOML file read at startup because console-ui is std-only (a TOML parser would be its first
dependency) and a bad edit should fail the build, not the live page.

## Design
- Server-rendered HTML + CSS from Rust, **no JavaScript**, no inline `style` (the CSP `default-src 'none'; style-src 'self'` holds);
  SVG colours come from classes in the sheet.
- Dark and light: with no `galion_theme` cookie the page follows `prefers-color-scheme` (applies to every console
  page now: `<html>` gets no `data-theme`, the sheet has a `@media (prefers-color-scheme:light)` block, and the
  header shows exactly one of "Use light"/"Use dark" via CSS). The cookie still wins.
- New text-safe tokens `--c-ok/--c-warn/--c-bad/--c-link` per theme (the old green/amber are too light on white).
- Responsive: one column on a phone, cards `auto-fill minmax(21rem)`, process diagram + chart side by side from 960 px.
- Accessible: landmarks and headings per section, SVGs are `role="img"` with `<title>`/`<desc>`, the numbers also exist as a real `<table>`.
- Cost: the page makes one API call (`GET /api/v1/health`, header dot). The page is ~31.7 KB; the sheet grew from ~9.3 KB to ~19.3 KB.

## Run and view
```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/landing-overview
cargo build --release -p console-static
$CARGO_TARGET_DIR/release/galion-console-static --ui rust --upstream 127.0.0.1:19401 --bind 127.0.0.1:19400
# open http://127.0.0.1:19400/console  (loopback host + loopback peer = no pairing needed)
```
With a real API, point `--upstream` at it (e.g. the console API on 8088) and the header dot turns "up".

Screenshots (headless Chrome, `--window-size=1280,3700` / `390,7700`, dark via `--force-dark-mode`):
`/workspace/swarm/artifacts/landing/{desktop-1280,phone-390}-{dark,light}.png`.
