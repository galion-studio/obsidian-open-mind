<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: nerve-console-blindspot-test (wave-4 slot 332, F-313-2 guard)

Branch `swarm/nerve-console-blindspot-test` off `swarm/nerve-planned-stop-tag`
12956b45 + cherry-pick of slot 315 `nerve-console-deep-health` 38750655 (the
deep route probe; clean pick, new SHA da45a86). Slot 315 is done, so the guard
is a normal (non-ignored) `GALION_REAL=1` test. No deploy, no live change.

## What the guard asserts

`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_console_blindspot`
boots REAL `galion-console-serve` binaries (no stub fallback) and a real
`openmind-nerve serve` on 127.0.0.1:22020 (ports 22020-22029; reflex,
rollback, systemd and journal off). The components file is the nerve's own
built-in `console` spec (`default_specs`: path `/`, HEPHAESTUS, group, `deep`
list) moved to box ports, so emptying `CONSOLE_DEEP` or dropping it from the
default spec also fails the guard.

| Console | Binary | Shape (direct GET) | Nerve must say |
|---|---|---|---|
| `console` :22021 | live build 2d846fb8 (`/opt/galion/rust/bin/galion-console-serve-2d846fb8`, sha256 `516556f946977d2d`, byte-identical to the VPS binary) `--local-api` | `/` 302, `/api/v1/admin` 404, `/api/v1/services` 404 | `degrading` (never `healthy`), both deep reasons, `consecutive_down` 0, deep records ok=false; `system.components_worst`, `system.status`, `/health.system`, gaia HEPHAESTUS not healthy; open **warn** incident carrying the console deep signal; dashboard shows the reason |
| `console-noadmin` :22022 | fixed build dfa120c6 (console-admin-lockout-fix, sha256 `76dba573529e5f88`) WITHOUT `--admin-store` | `/` 302, admin **503**, services 200 | `degrading`, only the admin reason |
| same, restarted WITH the store | dfa120c6 | admin 200 | back to `healthy` next cycle (not sticky) |
| `console-full` :22023 | dfa120c6 with `--admin-store/--admin-mirror` | all 200/302 | `healthy` control, no reasons |
| `console` killed | - | connection refused | `failing` (hard), no deep record, no deep reason |

Also: the admin store dirs hold only the pre-made `mirror/` folder after the
run (probes are GETs only).

## Results (box, Europe/Zagreb)

| Run | Time | Result |
|---|---|---|
| New nerve (this branch, release) + 2d846fb8 + dfa120c6 | 2026-10-28 01:27:32 +01:00 | **pass**, 15.5 s |
| Same, nerve swapped to the base build 12956b45 (`NERVE_BLINDSPOT_NERVE_BIN`, sha256 `49294098d8ac1aab`, no deep probe) | 2026-10-28 01:29:43 +01:00 | **FAILS as intended**: `F-313-2 REGRESSED: nerve reports "console" healthy while [("/api/v1/admin", 404), ("/api/v1/services", 404)] fail`, system `healthy`, 0 incidents |
| Blind console only (no fixed bin) | 2026-10-28 01:18:36 +01:00 | pass, 10.2 s |
| Not gated (no `GALION_REAL`) | 2026-10-28 01:29:48 +01:00 | skipped, ok |

New nerve, after 2 samples:

```
direct GET :22021 -> / 302, /api/v1/admin 404, /api/v1/services 404   (2d846fb8 = live)
direct GET :22022 -> / 302, /api/v1/admin 503, /api/v1/services 200   (dfa120c6, no store)
direct GET :22023 -> / 302, /api/v1/admin 200, /api/v1/services 200   (dfa120c6, store)
console          degrading http=302 consecutive_down=0 ["degrading: deep route /api/v1/admin HTTP 404","degrading: deep route /api/v1/services HTTP 404"]
console-noadmin  degrading http=302 ["degrading: deep route /api/v1/admin HTTP 503"]
console-full     healthy   http=302 []
system status=degrading components_worst=degrading; /health.system=degrading; HEPHAESTUS degrading
incident cause=console severity=warn components=["console","console-noadmin"]
console-noadmin restarted with store -> healthy (next cycle, ~5 s)
console killed -> failing "health probe down (connect: Connection refused)", deep=null
```

Old nerve 12956b45, same consoles: all three `healthy`, `deep=null`,
system `healthy`, score 100, 0 open incidents = the blind spot.

## VPS read-only probe (live, untouched)

2026-10-28 01:18:57 +01:00, over ssh, GETs only:

```
live :8080  /  302   /api/v1/health 200   /api/v1/admin 404   /api/v1/services 404
live nerve  /opt/galion/rust/bin/openmind-nerve-9d1a4f3, :8670 /api/v1/state:
  console status=healthy http=302 reasons=[] deep=null   <- F-313-2 still live
  HEPHAESTUS healthy; system degraded (other causes: galion-one-mouth, cloudflared, tuner-quick)
systemctl is-active console.service galion-nerve.service -> active active
```

So the live nerve still has the blind spot until a nerve build with the slot
315 probe is deployed (D-315-2); this guard then keeps it fixed.

Side observation (N-332-1, cause not checked): the live nerve `/api/v1/state`
`cycle` went 36 (ts 01:15:01) -> 37 (/health ts 01:22:58) although
`interval_s` is 30, i.e. one full cycle per ~8 min (37 cycles since start
2026-10-27 18:15 +01:00). VPS load average 12.9/14.3/15.4 at 01:22.

## Gates

2026-10-28 01:30-01:45 +01:00 on the box:

- `cargo fmt --all --check`: ok.
- `clippy --workspace --all-targets -D warnings` (excl. openmind-sandbox, cgroup-detect): ok (after the allow below).
- `cargo test --workspace --no-fail-fast` (excl. openmind-sandbox, cgroup-detect, hermes-tools): 273 suites, 1309 passed, **2 failed**, 6 ignored. Both failures are not from this branch:
  - `openmind-nerve --test cortex_replay` `replay_matches_the_pre_trait_build`: fails identically on the untouched base 12956b45 (stale golden; fixed on the nerve-merge-4 line, tuner-quick baseline_buckets 0->168). Left for the nerve merge slot.
  - `room --lib` `default_and_fixture_do_not_open_a_socket`: process-wide socket fd count flake under parallel load; 3/3 pass when re-run alone.
- x86_64 + aarch64 musl release build (rust-lld) ok (x86_64 musl nerve sha256 `3531cacc04c5dada`); `cargo check` aarch64-linux-android + armv7-unknown-linux-musleabihf ok.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 533 scanned, 0 missing.

Clippy note: workspace clippy on the base 12956b45 already fails with
`large_enum_variant` on `evt::journal::Msg` (serde_json `preserve_order`
feature unification); this branch adds the same documented
`#[allow(clippy::large_enum_variant)]` that nerve-merge-4 43c3d29 uses.

## Reproduce

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/nerve-console-blindspot-test
# fixed console: cargo build --release -p console-static --bin galion-console-serve in wt/console-admin-lockout-fix (dfa120c6)
GALION_REAL=1 NERVE_BLINDSPOT_CONSOLE_FIXED_BIN=<dfa120c6 galion-console-serve> \
  cargo-slot test -j 2 --release -p openmind-nerve --test real_console_blindspot -- --nocapture
# prove the guard: NERVE_BLINDSPOT_NERVE_BIN=<pre-315 openmind-nerve> must FAIL
```
