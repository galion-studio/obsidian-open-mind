<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: mouth-loading-diag (swarm slot 181, ports 20510-20519)

Test: `crates/galion-one/tests/real_mouth_loading.rs` (Rust, gated by `GALION_REAL=1`). It starts the real
galion-one binary on loopback spare ports, plus an in-test fake voice service on 127.0.0.1:20512 (stands in for
galion-voice-serve, no models), polls `:mouth/health` repeatedly, `POST /synthesize`, and kills everything.
Optional `GALION_ONE_LIVE_BIN` = a copy of the live VPS binary (taken with a read-only `scp` of
`/opt/galion/rust/bin/galion-one`, sha256 prefix `24cb4976`, = swarm/one-no-shellout 40454f9).

```
GALION_REAL=1 GALION_ONE_LIVE_BIN=/workspace/swarm/scratch/181/galion-one-live-40454f9 \
  cargo test --release -j 2 -p galion-one --test real_mouth_loading -- --nocapture
```

## Run 1, 2026-10-21 08:59 +02:00 (release build of this branch, integration-4 42de672 + test)
```
[A-branch-no-mouth] up in 101 ms
[A-branch-no-mouth] GET /health x6 over 6s -> 503 {"ok":false,"status":"unavailable","provider":"unavailable","loaded":false,"reason":"no speech backend configured; set GALION_MOUTH to a galion-voice-serve address"}
[A-branch-no-mouth] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[B-branch-remote-up] GET /health x3 over 2s -> 200 {"ok":true,"status":"ok","provider":"fake-voice-serve","slot":181}
[B-branch-remote-up] POST /synthesize -> 200 audio/wav 4844 B in 0 ms
[C-branch-remote-down] GET /health x3 over 2s -> 503 {"ok":false,"status":"unreachable","provider":"remote","mouth":"127.0.0.1:20513"}
[C-branch-remote-down] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[L1-live-no-mouth] up in 100 ms (galion-one-live-40454f9)
[L1-live-no-mouth] GET /health x6 over 6s -> 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
[L1-live-no-mouth] POST /synthesize -> 500 {"error":"voice"} in 0 ms
[L2-live-remote-up] GET /health x3 over 2s -> 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
[L2-live-remote-up] POST /synthesize -> 200 audio/wav 4844 B in 0 ms
[L2-live-remote-up] fake mouth hits during case: 2
test real_mouth_loading_diag ... ok   (14.53 s)
```
Runs 2 (08:59:47 +02:00) and 3 (09:00:01 +02:00): same output, `ok`, 14.53 s / 14.52 s, L2 fake hits 2 each.
Without `GALION_REAL=1` the test prints "skipped" and passes in 0.00 s.

What it proves:
- The "loading" body is a constant: 6 polls over 6 s give byte-identical 503s, and `/synthesize` fails in 0 ms.
  Nothing is loading (L1, and identical to the VPS answer).
- On the live binary a working remote mouth gets `/synthesize` traffic (200 WAV) but `/health` stays 503
  "loading" (L2): setting `GALION_MOUTH` alone cannot turn the nerve row healthy.
- The integration-4 binary tells the truth in all three set-ups (A/B/C).

No leftover processes (`pgrep` for 2051x ports: none). Temp dirs removed by the test's Drop guard.

## Live VPS comparison (read-only GETs over ssh)
| Time (+02:00) | `GET 127.0.0.1:8002/health` | nerve cycle | `galion-one-mouth` |
|---|---|---|---|
| 2026-10-21 08:56:53 | 503 `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}` (0.8 ms) | 7004 | degrading, `health probe HTTP 503`, since 2026-10-18T22:35:04+02:00 |
| 2026-10-21 09:00:33 | same 503, same body | 7011 | same |

Live = box case L1 byte for byte. Cortex not live (8680/8690 no answer); the nerve was the real-time source.

## Checks on this branch
- `cargo fmt --check -p galion-one`: ok
- `cargo clippy -p galion-one --all-targets -- -D warnings`: ok, 0 warnings
- `cargo test -p galion-one`: unit 39/39, real_mouth_loading 1/1 (gated), real_one_probe 1/1 (gated)
- `scripts/no-python.sh`: ok
- No new dependencies (test uses std only).
