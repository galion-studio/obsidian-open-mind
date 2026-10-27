<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests — android-admin-plan (wave-4 slot 307)

All times Europe/Zagreb, CET +01:00. Box only; VPS touched read-only (GET + `systemctl status`
through an ssh forward that was closed after each run). §11: no device was flashed or touched.

## VPS probe (read-only)

| Time (+01:00) | Check | Result |
|---|---|---|
| 21:02:11 | `systemctl status` console | active, MainPID 240630, `galion-console-serve-2d846fb8 --ui rust --local-api` on :8080 |
| 21:02 | `GET /api/v1/health` | 200, implementation rust (local-api) |
| 21:02 | `GET /console` | 503 "Console API is not answering" |
| 21:02 | `GET /api/v1/admin`, `/admin`, `/pair` | 404 |
| 21:02 | `GET /session` | 503 ; `GET /api/v1/session-pack` 401 |

Finding: the phone pairing gate (`/api/v1/admin`) the console-ui expects is not served by the live
Rust local-api. Fails closed (no access), but phones cannot pair. Nerve still marks the console
healthy because it checks the port only → proposal N-307-1 in `docs/android-admin/PLAN.md`.

## Box tests

| Time (+01:00) | Command | Result |
|---|---|---|
| ~21:00–21:14 | `cargo-slot test -j 2 --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 388 suites, 2041 passed, 0 failed, 18 ignored, EXIT=0 |
| 21:14 | `cargo fmt --all --check` | ok |
| 21:14 | `cargo-slot clippy -p galion-admin-console -p gaia-core -p gaia --all-targets -- -D warnings` | ok |
| 21:14 | clippy `--target aarch64-linux-android` and `armv7-unknown-linux-musleabihf`, `-D warnings` | ok |
| 21:14 | `cargo-slot test -p galion-admin-console -p gaia-core -p gaia` | 102 passed, 0 failed (crate lib 15/15) |
| 21:14 | `GALION_REAL=1 cargo-slot test --release -p galion-admin-console --test real_admin_console` (fake nerve 21771 / console 21772, server 21770) | 1 passed |
| 21:14 | release musl builds | x86_64 578336 B, aarch64 506512 B; `--version` ok |
| 21:15 | `scripts/no-python.sh`, `spdx-scan`, `repo-check` | ok / missing 0 / PASS 0 errors |

aarch64 binaries were built and clippy-checked but not executed (no qemu on the box).

## Live run (real nerve + real console, read-only)

`ssh -N -L 127.0.0.1:21773:127.0.0.1:8670 -L 127.0.0.1:21775:127.0.0.1:8080 slavik-vps`, then
`GALION_REAL=1 GALION_ADMIN_REAL_NERVE=127.0.0.1:21773 GALION_ADMIN_REAL_CONSOLE=127.0.0.1:21775`.

- 21:09:17–21:09:25: pass; nerve cycle 19, score 44 grade F; 4 open incidents.
- 21:15:18–21:15:26: pass (after the console-ui JSON contract was added); startup 50 ms;
  nerve ts 21:15:01+01:00 cycle 20 mode live score 64 grade D; components healthy 10,
  degrading 1, failing-soon 1, failing 1, absent 1; attention galion-one-mouth degrading,
  cloudflared failing-soon, tuner-quick failing, voice-worker absent; open incidents
  inc-06ffb32f error edge, inc-865945dd error galion-one; console http 200 healthy rust;
  audit chain ok, 16 records. Forward closed, 0 listeners left on 2177x.

Steps covered: unpaired 401, one-time code issue, foreign Origin 403, bad code 401, form pair 303 +
cookie, paired status, page render, used code refused, JSON `POST /api/v1/admin` console-ui
contract (`{"you":true}` + cookie), devices CLI, lockout 429 after 5 fails, 8192-byte body 413,
revoke → 401, audit verify + no token/code in log + tamper → exit 4, file modes 0700/0600,
0.0.0.0 bind and non-loopback nerve refused (exit 2).

## Process note

One shell command by mistake contained a `python3` call; the `/workspace/swarm/bin/python3` shim
blocked it (exit 127). No Python ran.
