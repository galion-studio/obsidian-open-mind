<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-pairing-e2e (wave 4 slot 286)

Branch `swarm/console-pairing-e2e`. Europe/Zagreb (CET +01:00), 2026-10-27.
**Box only. No VPS writes. No console.service change. No live cortex/console deploy.**

## Base choice

| Candidate | SHA | Used? | Why |
|---|---|---|---|
| `swarm/console-session-pack` (slot 265) | `21fd3baafe9c` | **YES** | Follow-up target: already has Android `galion_device` resume + session-pack proxy harden. |
| `swarm/console-v2-sprite-health` | `58726e7c72cc` | no | Sibling of session-pack from `console-v2-audit`; sprite/health not required for pairing e2e. |
| `swarm/main-staging` | `98466ab9f33d` | no | Does **not** contain console-v2-audit / session-pack harden; merging would drop the code under test. |

Merge-base of session-pack and sprite-health: `2a012311` (`console-v2-audit`). E2e sits on session-pack tip only.

## What is tested

Release binary `galion-console-static --ui rust` (Rust console-ui) on the box
with a fake loopback console API. Fixture tokens only (same shapes as
`crates/console-static/tests/ui.rs`); no live secrets.

| Step | Expect |
|---|---|
| GET `/api/v1/session-pack` unpaired | 401 `phone required` JSON |
| POST `/_ui/pair` wrong code | 401 pairing form error |
| POST `/_ui/pair` CSRF cross-site | 403 |
| POST `/_ui/pair` good code (`back=/session`) | 303 → `/session`; `galion_admin` + `galion_device` HttpOnly; Secure on galion.app host; Max-Age=31536000; stray upstream cookies not relayed |
| GET `/api/v1/session-pack` with admin cookie | 200; `Content-Type: application/zip`; `Content-Disposition: attachment; filename="build-session.zip"`; `Cache-Control: no-store`; `X-Content-Type-Options: nosniff`; body starts `PK` |
| GET `/session` with device cookie only | 303 resume + `galion_admin` set |
| GET `/session` with refused device | 200 pairing form; `galion_device` cleared Max-Age=0 |
| POST `/api/v1/session-pack` | 405 |

## Ports

| Role | Addr |
|---|---|
| Front (release binary) | `127.0.0.1:21560` |
| Fake upstream API | `127.0.0.1:21561` |
| Reserved | 21560-21569 |

## Command

```
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/console-pairing-e2e
export CARGO_INCREMENTAL=0
GALION_REAL=1 cargo-slot test --release -p console-static --test real_pairing_e2e -j 2 -- --nocapture
```

## Evidence

Filled after the GALION_REAL run (Zagreb time in CALL-RECORD / DONE line).

## Evidence (box, 2026-10-27 17:57:39 +0100 Europe/Zagreb CET +01:00)

```
GALION_REAL=1 cargo test --release -p console-static --test real_pairing_e2e -j 2 -- --nocapture

NEG unpaired GET /api/v1/session-pack -> 401 ("{\"error\":\"phone required\"}")
NEG POST /_ui/pair code=WRONG -> 401
NEG POST /_ui/pair CSRF cross-site -> 403
OK pair -> 303 Location:/session admin+device HttpOnly Secure
OK paired GET /api/v1/session-pack -> 200 zip 11 bytes disposition=build-session.zip
OK device resume GET /session -> 303 + galion_admin
NEG refused device -> pairing page + Max-Age=0 clear
NEG POST /api/v1/session-pack -> 405
real_pairing_e2e PASS on 127.0.0.1:21560 upstream=127.0.0.1:21561 in 4 ms (fixture tokens only; no VPS)
test real_android_pairing_and_session_pack_e2e ... ok
test result: ok. 1 passed; 0 failed; 0 ignored
```

Gates (same box turn): `cargo fmt --check` ok on new test; `cargo clippy -p console-static -p console-ui --all-targets -- -D warnings` ok; `cargo test -p console-static -p console-ui` green (incl. `android_device_resume_and_session_pack_gate` + `pairing_form_flow`); `scripts/no-python.sh` ok. No new deps.
