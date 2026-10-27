<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve console deep health (wave-4 slot 315, N-307-1 / F-313-2)

Branch `swarm/nerve-console-deep-health`, based on `swarm/nerve-merge-4`
`5dac9b4a`. No deploy: the live nerve (`openmind-nerve-9d1a4f3`) and the live
console were not touched.

## Problem

The nerve probes the console with `GET /` on :8080. The live Rust serve
(`galion-console-serve-2d846fb8 --local-api`) answers `/` with 302 while
`/api/v1/admin` and every board route are 404, so the nerve has said
`console: healthy` since the Rust cutover although remote console pages are
503 and phones cannot pair.

## Change (crate `openmind-nerve`, std + serde_json only)

- `Spec::deep: &'static [&'static str]`: extra loopback GETs run only after the
  health path passes. Built-in table: `console` gets
  `CONSOLE_DEEP = ["/api/v1/admin", "/api/v1/services"]`; every other
  component has none (unchanged behaviour).
- `NERVE_COMPONENTS` JSON accepts `"deep": ["/path", ...]` (max 8, must start
  with `/`, printable ASCII without spaces, max 128 bytes, so no CR/LF can
  reach the request line).
- `probe::deep_route_ok`: 2xx/3xx pass; 401/403/429 pass (route exists, a gate
  or lockout answered); 404, 405, other 4xx, every 5xx (admin "not
  configured" 503) and no answer fail. Timeout 2 s per route.
- `collect`: results go to `extra.deep` = `[{path, http, ok, ms, error}]`
  (visible in `/api/v1/state` and replayable from captures).
- `state::assess`: each failing route adds `degrading: deep route <path> HTTP
  <code>` (or `... down`). Never hard down and never counts toward
  `consecutive_down`, so the restart reflex cannot loop on a missing route.

## Results

| Test | When (+01:00) | Result |
|---|---|---|
| `cargo test -p openmind-nerve --lib` (new: `console_deep_routes_404_degrade_not_hard`, `deep_route_verdicts`, deep cases in `components_file_and_access_logs`) | 2026-10-27 22:29 | 114 passed, 0 failed |
| `GALION_REAL=1 cargo test --release -p openmind-nerve --test real_console_deep` (stub consoles, nerve :21850, consoles :21851/:21852) | 2026-10-27 22:30 | pass, 10.7 s |
| same with the REAL console binaries: `NERVE_DEEP_CONSOLE_BLIND_BIN` = galion-console-serve built from `2d846fb8` (the live build, sha256 prefix `f5b267abd029e721`), `NERVE_DEEP_CONSOLE_FULL_BIN` = built from `9c0f9d1a` (board API + admin gate, `274b9439c5641526`), `--dist` from the console-dist static fixture | 2026-10-27 22:31 | pass, 10.6 s |
| Extra box check: `9c0f9d1a` without `--admin-store` (slot 313 case C, binary-only swap) on :21853 | 2026-10-27 22:31 | `/` 302, `/api/v1/admin` **503**, `/api/v1/services` 200 → would be flagged degrading |
| VPS throwaway (read-only): musl `openmind-nerve` (sha256 prefix `236ea5b13656e34d`) as `nobody` in `/var/tmp/nerve-deep-315-20261027223157`, `NERVE_ADDR 127.0.0.1:21850`, `NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0 NERVE_ROLLBACK=0 NERVE_EVT_ESCALATE=off`, components = live console :8080 with and without deep; 13 s, killed, folder removed, :21850 closed | 2026-10-27 22:32 | see below |
| Workspace: `cargo fmt --all --check`; `clippy --workspace --all-targets -D warnings` (excl. openmind-sandbox, cgroup-detect); `cargo test --workspace` (excl. openmind-sandbox, cgroup-detect, hermes-tools) | 2026-10-27 22:36 | ok; 276 suites, 1332 passed, 0 failed, 6 ignored |
| `aarch64-unknown-linux-musl` release build (rust-lld), `cargo check` aarch64-linux-android + armv7-unknown-linux-musleabihf; `scripts/no-python.sh`; `scripts/spdx-scan.sh` | 2026-10-27 22:37 | ok; no-python ok; spdx missing 0 |

Box real test with the real binaries (nerve `/api/v1/state`, after 2 samples):

```
direct GET :21851/ -> 302   /api/v1/admin -> 404   /api/v1/services -> 404   (2d846fb8 = live)
direct GET :21852/ -> 302   /api/v1/admin -> 200   /api/v1/services -> 200   (9c0f9d1a)
console-blind        degrading  http=302  reasons=["degrading: deep route /api/v1/admin HTTP 404","degrading: deep route /api/v1/services HTTP 404"]  consecutive_down=0
console-blind-nodeep healthy    http=302  reasons=[]   (old behaviour = the blind spot)
console-full         healthy    http=302  deep: admin 200 ok, services 200 ok
console-full killed -> failing "health probe down (connect: Connection refused)" after 5.0 s, no deep record
admin store dir after probes: only the pre-made "mirror" folder (GETs wrote nothing)
```

VPS, same moment, live console :8080:

```
new nerve (throwaway)  console        degrading  ["degrading: deep route /api/v1/admin HTTP 404","degrading: deep route /api/v1/services HTTP 404"]
new nerve (throwaway)  console-nodeep healthy    []
live nerve :8670/state console        healthy    []          (openmind-nerve-9d1a4f3)
systemctl is-active console.service galion-nerve.service -> active active (untouched)
```

Earlier read-only probe 2026-10-27 22:26 +01:00: live :8080 `/` 302,
`/api/v1/health` 200, `/api/v1/admin` 404, `/api/v1/services` 404,
`/api/v1/catalog` 404; live nerve console `healthy`, `reasons: []`.

## Owner decisions

- **D-315-1 severity**: deep failures are `degrading` (warn incident, no
  restart). Option: raise `/api/v1/admin` failure to `failing` for the console
  (still non-hard). Recommendation: keep degrading; a restart cannot fix it.
- **D-315-2 deploy**: ship with the next nerve build. Expect the live console
  to show `degrading` until D-303-1 / D-313-1 (board API + admin store
  drop-in) are deployed; with 9c0f9d1a and the store flags both routes are
  200 and the console returns to `healthy`.
- **D-315-3 more routes**: only two cheap GETs per cycle now. Adding
  `/api/v1/catalog` or HTML pages under Host `console.galion.app` would catch
  the remote 503 page gate too, but needs a Host override in the probe.

## Reproduce

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/nerve-console-deep-health
GALION_REAL=1 cargo-slot test -j 2 --release -p openmind-nerve --test real_console_deep -- --nocapture
# with real consoles:
GALION_REAL=1 NERVE_DEEP_CONSOLE_BLIND_BIN=<2d846fb8 galion-console-serve> \
  NERVE_DEEP_CONSOLE_FULL_BIN=<9c0f9d1a galion-console-serve> \
  CONSOLE_DIST_STATIC=/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static \
  cargo-slot test -j 2 --release -p openmind-nerve --test real_console_deep -- --nocapture
```
