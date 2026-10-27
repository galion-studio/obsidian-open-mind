<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-v2-sprite-health (wave 4 slot 276)

- Branch: `swarm/console-v2-sprite-health`
- Base: `swarm/console-v2-css-budget` 34436c9
- Zone: Europe/Zagreb (CET +01:00)
- Constraint: box tests only; NO VPS; NO touch live console.service; ports 21460-21469
- Third-party: **none** (sprite cut alone; R4 `miniz_oxide` gzip not taken — still an open owner ruling)

## What changed
1. **Icon sprite**: `icons::PATHS` rendered once as `/_ui/i.<fnv64>.svg` (`<symbol id=…>`), served with `Cache-Control: public, max-age=31536000, immutable`. `icons::icon` emits `<svg…><use href="/_ui/i.<h>.svg#name"/></svg>` instead of inlining `<path d=…>`.
2. **Cached health**: `Ui::cached_health` + `HEALTH_CACHE_TTL` (5 s), `Mutex` + `Instant`, single-flight. Header origin-dot and rooms that read `/api/v1/health` reuse one upstream GET (`origin`, `status`, `read_room`, `agents`, `port5` node).

## Box real evidence (2026-10-27 17:53:00 +0100 Europe/Zagreb)
```
GALION_REAL=1 cargo test --release -p console-static --test real_sprite_health -- --nocapture
# bind 127.0.0.1:21460, fake upstream 127.0.0.1:21461
health upstream hits after burst: 1 (want 1)
  # first GET /console fills cache; then 3× each of
  # /data /agents /console /gateway /memory → still 1 hit
GET /services -> 200, 14512 bytes, 0 ms; sprite /_ui/i.ccc436393bd6554f.svg
GET /_ui/i.ccc436393bd6554f.svg -> 200, 5079 bytes, 46 symbols, immutable
```

Also re-checked:
```
GALION_REAL=1 cargo test --release -p console-static --test real_landing -- --nocapture
GET /console -> 200, 31678 bytes, 1 ms, 0 <script>, 9 cards, 2 svg
GET /_ui/console.43dced0d8441d776.css -> 200, 20324 bytes (base)
GET /_ui/overview.d30a957c27693259.css -> 200, 6040 bytes (overview)
GET /hermes -> 200
```

Unit/integration: `cargo test -p console-ui -p console-static` green (incl. `sprite_and_health_cache`: 8×/data + 8×/console → 1 health GET; chrome `<use href="/_ui/i.…">`).
Clippy `-D warnings` on those crates: 0. `cargo fmt --check` ok. `scripts/no-python.sh` ok.
No new third-party deps. No VPS contact. Live console.service not touched.

## Note on HTML byte cut
Path data in `PATHS` averages ~45 B, so per-icon HTML with a full `/_ui/i.<16 hex>.svg#id` href is close to the old inline `<path>`. The wins are (a) one immutable sprite fetch shared across navigations, (b) health RTT removed from chrome renders after warm-up. Larger HTML cuts still need R4 gzip and/or fragment-only `<use href="#id">` with an in-document sprite (CSP-friendlier on Firefox for external `<use>`).

## Owner decisions
- **R4** HTML gzip / `miniz_oxide`: not added (task: prefer sprite cut; needs owner yes).
- No live console.service / VPS write / CF DNS / HADES.
