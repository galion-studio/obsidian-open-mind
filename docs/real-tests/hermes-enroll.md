<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: Hermes device enrollment (swarm/hermes-enroll-2, resumed from swarm/hermes-enroll)

| Field | Value |
|---|---|
| Run | 2026-10-21T00:59:53+02:00 to 00:59:54+02:00 (Europe/Zagreb), on the build box (wave 4, slot 102); 47 s for the whole test incl. the two omnode release builds |
| Test | `crates/hermes-gateway/tests/real_enroll.rs`, a Rust test that builds, starts, probes and tears down (`GALION_REAL=1 cargo test --release -p hermes-gateway --test real_enroll -- --nocapture`). It replaces `scripts/real/hermes-enroll.sh` (deleted; Rust-only rule 2026-10-20 20:41 CEST +02:00) |
| Binaries | hermes-gateway: the release binary cargo builds for the test (host x86_64); omnode A: release build of edge/openmind-node made by the test (host x86_64); omnode B: aarch64 musl release (rust-lld) under `qemu-aarch64-static` |
| Processes | `hermes-gateway --mock` (offline mock provider: no network, no spend) on 127.0.0.1:19720; `omnode boot` A on 127.0.0.1:19721 and B on 127.0.0.1:19722, each with its own state dir; refused-start probes on 19723 (they exit before binding) |
| Ports | 19720-19723 only (slot 102 range 19720-19729), loopback only; the test asserts all four are free before start and after teardown |
| Result | **72 passed, 0 failed** (the 67 checks of the old script + 1 "no token or owner key in any Hermes log" + 4 "port free after teardown") |

Secrets: the owner API key is a throwaway random value made per run (from
/dev/urandom); it and the node credentials stay in the test process's memory, never
in argv or output. The test asserts that A's token is not in omnode's stdout/stderr,
not in the Hermes logs, not in the Hermes store (only its SHA-256 is) and not in the
owner's device list, and that neither token nor the owner key is in any Hermes log.
Device ids and fingerprints are public identifiers of temporary identities that were
deleted with the work dir.
## What was exercised

1. Enrollment off by default: `/openmind/*` is 404.
2. Unsafe configs refuse to start (exit 2, before binding): no URL, plain http to a
   public host, wrong path, non-loopback bind without `ALLOW_PUBLIC`, nonce TTL
   shorter than the payload life.
3. Hermes with enrollment on + two real omnode nodes (A x86_64, B aarch64/qemu).
4. A: `omnode enroll request` -> 202 pending; owner lists pending (401 without key);
   pending fingerprint equals `omnode identity show`; claim before approval -> pending,
   no credential; approve with a wrong fingerprint -> 409; approve -> A advertised
   `revolut-pay`, Hermes granted `cpu,sandbox` and denied `revolut-pay`; A claims the
   credential (file mode 600); whoami and scope `cpu` accepted, scope `revolut-pay` 403;
   A's token refused on the owner API (`/v1/models`) and on owner enroll routes; the
   normal owner API still works (mock chat 200).
5. Refusals: replayed nonce (409), expired payload made with `omnode pair --now
   <1000 s ago>` (400), tampered capabilities / expires_unix / public key / enroll_url /
   unknown field (400), forged token and no token (401), claim by a device with no
   request, claim while pending (202, no token), claim after the owner rejected it (403).
6. B enrolls properly with scopes `[cpu]`; B accepted, `sandbox` not granted.
7. Revoke A: A's credential refused (401), new claim and re-enrollment refused
   (`device revoked`); B unaffected.
8. Restart Hermes: A still refused, B's credential still accepted, the replayed nonce
   still refused, device list `A:revoked B:active`, store mode 600.
9. Damaged store: corrupt JSON and mode 644 both refuse to start, the corrupt file is
   left byte-identical; a store in a missing directory refuses to start.

## Output (work dir shown as `$WORK`)

```
== hermes-enroll real-life test 2026-10-21T00:59:53+02:00
  hermes-gateway 1836072 bytes (cargo test release profile)
  omnode A x86_64 1462408 bytes; omnode B aarch64 (qemu-user)
== 1. enrollment is off by default 2026-10-21T00:59:53+02:00
  PASS off: GET /openmind/node/whoami (404)
  PASS off: POST /openmind/enroll (404)
== 2. unsafe enrollment config refuses to start 2026-10-21T00:59:53+02:00
  PASS no URL: exit 2, "HERMES_RS_ENROLL_URL is not set"
  PASS plain http to a public host: exit 2, "plain http is only allowed for a loopback"
  PASS wrong path: exit 2, "path must be /openmind/enroll"
  PASS non-loopback bind: exit 2, "needs HERMES_RS_ENROLL_ALLOW_PUBLIC=1"
  PASS nonce TTL shorter than payload life: exit 2, "NONCE_TTL must be"
== 3. hermes (enroll on) + two omnode nodes 2026-10-21T00:59:53+02:00
  log: enroll on: http://127.0.0.1:19720/openmind/enroll store $WORK/hermes/rs-enroll.json
  PASS store created, mode (600)
  PASS omnode on 19721 up (yes)
  PASS omnode on 19722 up (yes)
  node A omn-9e7c70e3c3f5 SHA256:9e7c-70e3-c3f5-3d4a-36e4-aed5-7515-0d02
  node B omn-db8e7c9ccf10 SHA256:db8e-7c9c-cf10-14c2-f137-7d11-48fc-00c6 aarch64
== 4. node A enrolls, owner approves by fingerprint, A claims 2026-10-21T00:59:54+02:00
  PASS A: enroll request -> pending (exit 3) (3)
  PASS A: hermes answered (202 pending)
  PASS pending list without owner key (401)
  PASS pending list (owner) (200)
  PASS pending fingerprint == omnode identity show (SHA256:9e7c-70e3-c3f5-3d4a-36e4-aed5-7515-0d02)
  PASS A: claim before approval -> pending (exit 3), no credential (3 absent)
  PASS approve with wrong fingerprint (409)
  PASS approve without owner key (401)
  PASS approve (owner) (200)
  PASS A: scopes granted / denied (cpu,sandbox / revolut-pay)
  PASS A: claim -> credential (exit 0) (0)
  PASS A: credential file mode (600)
  PASS A token not printed by omnode or hermes
  PASS hermes store holds no clear token
  PASS A: status (whoami) accepted (0)
  PASS A: scope cpu (0)
  PASS A: scope revolut-pay refused (2 true)
  PASS node token on the owner API (/v1/models) (401)
  PASS node token on owner enroll routes (401)
  PASS owner API unchanged: mock chat with owner key (200)
== 5. refusals: replayed nonce, expired, tampered, unenrolled 2026-10-21T00:59:54+02:00
  PASS B: first POST of its payload (202 pending)
  PASS B: same payload replayed (409 nonce already used)
  PASS B: expired payload (created 1000 s ago) (400 true)
  PASS tampered capabilities (400 signature does not verify)
  PASS tampered expires_unix (400)
  PASS foreign public key (400)
  PASS payload for another server (400)
  PASS unknown field (400)
  PASS unenrolled: forged token (401 not enrolled)
  PASS unenrolled: no token (401)
  PASS unenrolled B (no request file yet) cannot claim (2)
  PASS pending B: claim -> pending (exit 3) (3)
  PASS owner rejects B (200)
  PASS rejected B: claim refused (2 true)
  PASS B has no credential (absent)
== 6. node B enrolls properly (aarch64 (qemu-user)) 2026-10-21T00:59:54+02:00
  PASS B: enroll request -> pending (3)
  PASS approve B with scopes [cpu] (200 cpu)
  PASS B: claim -> credential (0)
  PASS B: accepted (0)
  PASS B: sandbox not granted (2)
== 7. revoke A 2026-10-21T00:59:54+02:00
  PASS revoke without owner key (401)
  PASS revoke A (owner) (200)
  PASS A's credential refused after revoke (401)
  PASS A: omnode status refused (2)
  PASS A: new claim refused (2 true)
  PASS A: re-enroll refused (2 true)
  PASS B unaffected (200 omn-db8e7c9ccf10)
== 8. restart hermes: persistence 2026-10-21T00:59:54+02:00
  PASS after restart: A still refused (401)
  PASS after restart: B's credential still accepted (200 cpu)
  PASS after restart: replayed nonce still refused (409 nonce already used)
  PASS devices after restart (omn-9e7c70e3c3f5:revoked omn-db8e7c9ccf10:active)
  PASS devices list shows no token
  PASS store mode after writes (600)
  store: nonces=9 pending=0 devices=2 flags=0
  PASS no token or owner key in any hermes log
== 9. damaged store refuses to start (never reset) 2026-10-21T00:59:54+02:00
  PASS corrupt store: exit 2, "is corrupt"
  PASS corrupt store left untouched (true)
  PASS store mode 644: exit 2, "must be 600"
  PASS store dir missing: exit 2, "write"
== hermes log lines (no secrets) ==
  2026-10-21T00:59:53+02:00 hermes-gateway enroll on: http://127.0.0.1:19720/openmind/enroll store $WORK/hermes/rs-enroll.json
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: pending omn-9e7c70e3c3f5 SHA256:9e7c-70e3-c3f5-3d4a-36e4-aed5-7515-0d02 caps=["cpu", "sandbox", "revolut-pay"]
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: approved omn-9e7c70e3c3f5 by slavik scopes=["cpu", "sandbox"] denied=["revolut-pay"]
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: credential issued to omn-9e7c70e3c3f5 (any earlier one replaced)
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: pending omn-db8e7c9ccf10 SHA256:db8e-7c9c-cf10-14c2-f137-7d11-48fc-00c6 caps=["cpu", "sandbox", "voice-client"]
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: replayed nonce from omn-db8e7c9ccf10
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: payload expired at 2026-10-21T00:53:14+02:00
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: signature does not verify
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: expires_unix does not match the signed expires
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: public_key is not an ed25519 point
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: signature does not verify
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: refused payload: payload rejected: unknown field `admin`
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: rejected omn-db8e7c9ccf10
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: claim from unenrolled device omn-db8e7c9ccf10
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: pending omn-db8e7c9ccf10 SHA256:db8e-7c9c-cf10-14c2-f137-7d11-48fc-00c6 caps=["cpu", "sandbox", "voice-client"]
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: approved omn-db8e7c9ccf10 by owner scopes=["cpu"] denied=[]
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: credential issued to omn-db8e7c9ccf10 (any earlier one replaced)
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: revoked omn-9e7c70e3c3f5 by slavik
  2026-10-21T00:59:54+02:00 hermes-gateway enroll on: http://127.0.0.1:19720/openmind/enroll store $WORK/hermes/rs-enroll.json
  2026-10-21T00:59:54+02:00 hermes-gateway enroll: replayed nonce from omn-db8e7c9ccf10
  PASS port 19720 free after teardown (true)
  PASS port 19721 free after teardown (true)
  PASS port 19722 free after teardown (true)
  PASS port 19723 free after teardown (true)
RESULT 2026-10-21T00:59:54+02:00 pass=72 fail=0
```

## Read-only probe of the live VPS (GET only, nothing written)

Hermes on the VPS has no enrollment route; it answers `/openmind/*` with 401 because
its auth check runs before routing. With enrollment off this branch answers 404 for
the whole `/openmind/` prefix (design §6, "route disabled: 404").

```
2026-10-20T20:50:28+02:00  ssh root@VPS (read-only), curl GET on 127.0.0.1
GET :8642/health                 -> 200   (live Python Hermes)
GET :8642/openmind/enroll        -> 401
GET :8642/openmind/node/whoami   -> 401
LISTEN 127.0.0.1:8642, 127.0.0.1:8652
2026-10-20T20:50:37+02:00
GET :8652/health -> {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}
GET :8652/openmind/enroll        -> 401   (live hermes-rs shadow, main code)
GET :8652/openmind/node/whoami   -> 401
```

## Other checks on this branch (same evening, box)

| Check | Result |
|---|---|
| `cargo fmt --check` (workspace and omnode) | ok |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok |
| omnode `cargo clippy --all-targets -- -D warnings` | ok |
| `cargo test --workspace --locked` | 509 passed, 0 failed (124 suites) |
| omnode `cargo test` | 74 + 1 passed |
| enroll unit tests (`hermes-rs` `enroll::tests`) | 8 passed |
| enroll HTTP tests (`hermes-rs/tests/enroll_http.rs`) | 2 passed |
| aarch64 musl release hermes-gateway | builds, 1,849,872 B, static |
| `cargo check --all-targets` armv7-unknown-linux-musleabihf and aarch64-linux-android (hermes-rs, hermes-gateway, omnode) | ok (omnode android: 2 dead-code warnings that exist on the base too) |
| `scripts/ucpu-gate.sh grep`, `scripts/spdx-scan.sh`, `scripts/no-python.sh` | ok, 0 missing, ok |

Re-run after the Rust port (2026-10-21 01:03 CEST +02:00, slot 102, through the
`cargo-slot` build queue, `-j 2`): `cargo fmt --all --check` ok;
`cargo clippy --workspace --all-targets --locked -- -D warnings` ok;
`cargo test --workspace --locked` 509 passed, 0 failed, 124 suites (the skipped
`real_enroll` counts as one pass without `GALION_REAL=1`, as the old wrapper did);
`scripts/no-python.sh` ok. omnode code is unchanged since f2121a8.
