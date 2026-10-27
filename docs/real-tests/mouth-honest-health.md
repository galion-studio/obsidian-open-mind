<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: mouth-honest-health (swarm slot 259, ports 21280-21289)

Test: `crates/galion-one/tests/real_mouth_honest_health.rs` (Rust, `GALION_REAL=1`).
Starts the real galion-one binary on loopback spare ports in **21280–21289**, plus
an in-test fake voice service on `127.0.0.1:21282`. Optional
`GALION_ONE_LIVE_BIN` = copy of the live VPS binary (lying `"loading"` stub;
`/workspace/swarm/scratch/181/galion-one-live-40454f9`, sha256 prefix `24cb4976`).

```
export CARGO_TARGET_DIR=/workspace/swarm/target/mouth-honest-health
export PATH=/workspace/swarm/bin:$PATH
GALION_REAL=1 GALION_ONE_LIVE_BIN=/workspace/swarm/scratch/181/galion-one-live-40454f9 \
  cargo-slot test --release -j 2 -p galion-one --test real_mouth_honest_health -- --nocapture
```

## Port map (slot 259)

| Port | Use |
|---|---|
| 21280 / 21281 | case A ONE_BIND / ONE_MOUTH_BIND (and L2 reuse) |
| 21282 | fake galion-voice-serve |
| 21283 | dead mind/ear/eye/memory + unreachable mouth |
| 21284 / 21285 | case B |
| 21286 / 21287 | case C |
| 21288 / 21289 | case L1 live binary |

## Run 1, 2026-10-27 17:38:33 CET (+01:00) (release, this branch)

```
[A-branch-no-mouth] up in 100 ms
[A-branch-no-mouth] GET /health x6 over 6s -> 503 {"ok":false,"status":"unavailable","provider":"unavailable","loaded":false,"reason":"no speech backend configured; set GALION_MOUTH to a galion-voice-serve address"}
[A-branch-no-mouth] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[B-branch-remote-up] GET /health x3 over 2s -> 200 {"ok":true,"status":"ok","provider":"fake-voice-serve","slot":259}
[B-branch-remote-up] POST /synthesize -> 200 audio/wav 4844 B in 0 ms
[C-branch-remote-down] GET /health x3 over 2s -> 503 {"ok":false,"status":"unreachable","provider":"remote","mouth":"127.0.0.1:21283"}
[C-branch-remote-down] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[L1-live-no-mouth] up in 100 ms (galion-one-live-40454f9)
[L1-live-no-mouth] GET /health x6 over 6s -> 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
[L1-live-no-mouth] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[L2-live-remote-up] GET /health x3 over 2s -> 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
[L2-live-remote-up] POST /synthesize -> 200 audio/wav 4844 B in 5 ms
[L2-live-remote-up] fake mouth hits during case: 2
test real_mouth_honest_health ... ok   (14.53 s)
```

Without `GALION_REAL=1` the test prints "skipped" and passes in 0.00 s.
No leftover listeners on 2128x after the run.

What it proves:
- This branch's honest path: unavailable / passthrough / unreachable (A/B/C).
- Live binary still hard-codes `"loading"` even when `GALION_MOUTH` works (L1/L2).

## Live VPS comparison (read-only, 2026-10-27 17:36 CET +01:00)

| Probe | Result |
|---|---|
| `GET 127.0.0.1:8002/health` | 503 `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}` (0.3 ms) |
| nerve `galion-one-mouth` | degrading, stuck loading ~4.8 d since 2026-10-22T23:46:47+02:00 |
| `:8003` / `:4003` | nothing listening (`voice.galion.app`→4003 CLOSED in tunnel) |

## Checks on this branch

- `cargo fmt --check -p galion-one`: ok
- `cargo clippy -p galion-one --all-targets -- -D warnings`: ok, 0 warnings
- `cargo test -p galion-one --bins`: 39/39
- `cargo test -p galion-one --tests`: gated reals skipped-ok; real with `GALION_REAL=1` 1/1
- `scripts/no-python.sh`: ok
- No new dependencies (test uses std only)
