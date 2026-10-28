<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-v2-css-budget (wave 4 slot 254)

- Branch: `swarm/console-v2-css-budget`
- Base: `swarm/console-v2-audit` 2a012311 (includes console-port-merge 508401dd)
- Zone: Europe/Zagreb (CET +01:00)
- Constraint: box tests only; NO VPS; NO touch live console.service; ports 21230-21239

## What changed
1. **R1 CSS budget split**: base sheet (`css::sheet`, RULES+PORT1-4+tokens+theme-toggle) <= 20 KiB; overview sheet (`css::overview_sheet`, `ov-*`) <= 8 KiB, linked only on `/console` as `/_ui/overview.<fnv64>.css`.
2. Light palette system path uses `:where(html:not([data-theme]))` under the prefers-color-scheme media query.
3. Dead duplicate PORT1 `.road` rules removed (PORT2 owns `.road` for cache+context).
4. **Drop dead SPA_FIRST for hermes**: `port2::SPA_FIRST` removed; `Ui::owns` no longer SPA-falls-back `/hermes`. `/arena` and `/bank` remain SPA-first.

## Box real evidence (2026-10-27 17:42:00 +0100 Europe/Zagreb)
```
GALION_REAL=1 cargo test --release -p console-static --test real_landing -- --nocapture
# bind 127.0.0.1:21232, dead upstream 127.0.0.1:21233
GET /console -> 200, 32013 bytes, 1 ms, 0 <script>, 9 cards, 2 svg
GET /_ui/console.43dced0d8441d776.css -> 200, 20324 bytes (base)   # <= 20480
GET /_ui/overview.d30a957c27693259.css -> 200, 6040 bytes (overview) # <= 8192
GET /hermes -> 200
```

Unit/integration: `cargo test -p console-ui -p console-static` green (incl. `spa_fallback_rust_owns_hermes`, CSS budget tests).
Clippy `-D warnings` on those crates: 0. `cargo fmt` ok.
No new third-party deps. No VPS contact. Live console.service not touched.
