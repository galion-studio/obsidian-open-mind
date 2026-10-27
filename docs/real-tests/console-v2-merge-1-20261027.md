<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-v2-merge-1 (wave 4 slot 300)

Branch `swarm/console-v2-merge-1`. Europe/Zagreb (CET +01:00), 2026-10-27.
**Prep tip only. Box only. No VPS writes. Not the live console cutover. No deploy.**

## What was folded

| Tip | SHA | How |
|---|---|---|
| `swarm/console-v2-sprite-health` (slot 276) | `58726e7c72cc` | base (branch start) |
| `swarm/console-pairing-e2e` (slot 286, carries session-pack harden slot 265 `21fd3baa`) | `45a0806d25d1` | `--no-ff` merge, **no conflicts** |

Merge-base of both: `2a012311` (`console-v2-audit`). 12 files from the pairing side
(console-ui `gate.rs`/`lib.rs`/pages, console-static proxy + tests, docs).
`console-rust-serve-impl` was not folded (not in row 300 scope).

## New test: `crates/console-static/tests/real_v2_merge.rs`

One release `galion-console-static --ui rust` process on `127.0.0.1:21700`,
fake loopback API on `127.0.0.1:21701` (ports 21700-21709). Fixture tokens only.
Runs the whole pairing + session-pack flow (8 steps from real_pairing_e2e), then on
the same paired process: cached health (TTL 5 s; 16 chrome pages -> 1 upstream
`/api/v1/health`), `/services` sprite `<use href="/_ui/i.*.svg">`, sprite asset
200 `image/svg+xml` immutable with >=40 symbols, paired `/session` 200 without
the pairing form.

Negative control: the same test on sprite-health-only `58726e7c` FAILS
(rc 101, panics at the pair step: no `galion_device` cookie), so the test proves
both halves are present.

## Evidence (box, 2026-10-27 20:40:35-20:41:00 +0100 CET)

```
GALION_REAL=1 cargo test --release -p console-static --test real_v2_merge \
  --test real_pairing_e2e --test real_sprite_health --test real_landing -- --nocapture --test-threads 1
real_landing_overview ok (/console 200 31678 B, 0 <script>, 9 cards)
real_android_pairing_and_session_pack_e2e ok (8/8 steps, 21560/21561)
real_sprite_and_cached_health ok (hits 1, sprite 5079 B 46 symbols immutable)
real_v2_merge_pairing_sprite_health ok:
  pairing 8/8; cached health 16 pages -> 1 hit; sprite /_ui/i.ccc436393bd6554f.svg 46 symbols;
  paired /session 200 14273 B; PASS in 5514 ms
```

Gates: `cargo fmt --all --check` ok; `cargo clippy -p console-static -p console-ui
--all-targets -- -D warnings` ok; `cargo test -p console-static -p console-ui`
23 suites 193 passed / 0 failed / 7 ignored (GALION_REAL-gated); `scripts/no-python.sh` ok;
`scripts/spdx-scan.sh` 247 scanned, 0 missing. No new deps. Sandbox crates not touched
(sandbox_vps_only: nothing to run on VPS for this slot).
