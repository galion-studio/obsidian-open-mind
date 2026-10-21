<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: omnode pairing + Hermes enrollment (swarm/omnode-enroll)

- Box only. Ports 22000 (main Hermes) and 22001 (Hermes with `HERMES_RS_ENROLL_PENDING_TTL=3`
  for the timeout case), both 127.0.0.1. No VPS write; no paid model (Hermes runs `--mock`).
- Command: `GALION_REAL=1 cargo test -j 2 -p hermes-gateway --test real_omnode_enroll -- --nocapture`
  (`crates/hermes-gateway/tests/real_omnode_enroll.rs`; skipped without `GALION_REAL=1`).
- Code under test: `swarm/omnode-enroll` fd63117 (base integration-5 0d68f46 + hermes-enroll-2).

| Run | When (+02:00) | Result |
|---|---|---|
| 1 | 2026-10-21 17:18:13 - 17:20:04 | 55 passed, 0 failed |
| 2 (final code) | 2026-10-21 17:35:05 - 17:37:15 | **55 passed, 0 failed** (log: `omnode-enroll/run-20261021-1735.log`) |

## What the test does

1. Builds `omnode` for the host (x86_64, "laptop") and for `aarch64-unknown-linux-musl`
   (static, rust-lld; the phone build) and starts two `hermes-gateway --mock` with
   `HERMES_RS_ENROLL=1` and a throw-away `API_SERVER_KEY` in a temp `HERMES_HOME`.
2. **Laptop**: `omnode pair http://127.0.0.1:22000/openmind/enroll` with a fresh state dir.
   It creates the key (mode 600), prints the QR and CODE, waits. The owner CLI
   `hermes-gateway enroll pending` shows the same CODE; `approve` with a wrong code fails (1);
   `approve <code in lower case>` succeeds. omnode stores the credential (600), opens the
   tunnel, Hermes verifies the device key; `enroll devices` shows connected + verified and no
   token; `enroll ping` returns an rtt (60 ms in run 2). `ss` shows **0 listening sockets** for
   omnode and exactly 1 outbound connection to Hermes.
3. **Refusal**: owner `reject CODE` → device exit 4 and told "refused".
   **Timeout** (22001, 3 s): device exit 5, told "timed out"; approving later fails; the
   owner list shows it as expired.
   **Signature / expiry / replay** (raw POSTs of `omnode pair --dry-run` documents):
   tampered payload 400, fresh 202, same payload again 409 "nonce already used", expired
   payload 400; tunnel without credential 401; owner route without key 401.
4. **Phone**: the aarch64 binary under `qemu-aarch64`, with `--via-curl` (the https code path:
   curl with `-K -` stdin config). Approve, tunnel UP, credential 600, ping rtt 67 ms (qemu),
   **0 listening sockets**.
5. **Restart**: Hermes restarted; the laptop notices, reconnects with backoff, device still
   active. **Revoke**: tunnel gets `bye`, omnode exits 6; `omnode tunnel` then exits 2.
6. **Teardown**: ports 22000/22001 free; neither Hermes log contains the token.

All 55 checks of run 2:

```
  PASS aarch64 phone binary is a static AArch64 ELF: true
  PASS laptop shows its CODE: true
  PASS code is XXXX-XXXX: true
  PASS QR is printed in the terminal: true
  PASS new device key, mode 600: true
  PASS owner pending exit: 0
  PASS owner sees the same code and device: true
  PASS approve with a wrong code: 1
  PASS owner approve CODE (typed lower case): 0
  PASS approve answer says approved: true
  PASS laptop: tunnel UP: true
  PASS credential file mode: 600
  PASS token never printed by omnode: true
  PASS laptop omnode listening sockets: 0
  PASS laptop omnode outbound connections to hermes: 1
  PASS owner devices: tunnel connected: true
  PASS owner devices: tunnel verified (device key): true
  PASS owner devices: no token or hash listed: true
  PASS owner ping over the tunnel exit: 0
  PASS ping has a round-trip time: true
  PASS laptop answered the challenge: true
  PASS pings flow: true
  PASS refused: CODE shown: true
  PASS owner reject CODE: 0
  PASS refused device exit code: Some(4)
  PASS refused device is told: true
  PASS timeout: CODE shown: true
  PASS unapproved device exit code (3 s window): Some(5)
  PASS timed-out device is told: true
  PASS owner can not approve after the timeout: true
  PASS owner list shows it as expired: true
  PASS dry-run document: true
  PASS tampered payload: 400 true
  PASS fresh payload: 202
  PASS same payload replayed: 409 nonce already used
  PASS expired payload: 400 true
  PASS tunnel without a credential: 401
  PASS owner route without the key: 401
  PASS phone: CODE shown: true
  PASS owner approves the phone: 0
  PASS phone: tunnel UP: true
  PASS phone credential mode: 600
  PASS owner ping to the phone: 0
  PASS phone (qemu) listening sockets: 0
  PASS laptop notices the restart: true
  PASS laptop tunnel back UP after restart (backoff reconnect): true
  PASS device still active after restart: true
  PASS owner revoke: 0
  PASS laptop exit after revoke: Some(6)
  PASS laptop is told it was revoked: true
  PASS `omnode tunnel` after revoke exit: Some(2)
  PASS port 22000 free after teardown: true
  PASS port 22001 free after teardown: true
  PASS hermes 22000 log has no token: true
  PASS hermes 22001 log has no token: true
```

## Unit tests and gates (2026-10-21, +02:00)

| Gate | Result |
|---|---|
| `cargo fmt --check` (workspace + edge/openmind-node) | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools -- -D warnings` | ok |
| `cargo clippy --all-targets -- -D warnings` in edge/openmind-node | ok |
| workspace tests (same excludes), 17:2x | **266 suites, 1260 passed, 0 failed, 6 ignored** |
| hermes-rs lib tests | 46 passed, of which **15 enroll tests** (7 new: pairing code shape + normalising, approve by code then claim + status, status poll signed + writes nothing, owner refusal told to device, unanswered request times out, old store opens, tunnel hello/proof/ping/replace/revoke) plus signature, expiry, nonce reuse, skew, URL binding from hermes-enroll-2 |
| omnode (edge/openmind-node) tests | 77 + 1 passed, 0 failed (new: code_matches_hermes, curl_config_quotes_and_keeps_secrets_off_argv) |
| x86_64-unknown-linux-musl release: hermes-gateway, omnode | ok (static-pie) |
| aarch64-unknown-linux-musl release: omnode; hermes-gateway `--no-default-features` (no SQLite C cross compiler on the box) | ok (static) |
| `cargo check` aarch64-linux-android, armv7-unknown-linux-musleabihf: omnode; hermes-gateway + hermes-rs `--no-default-features` | ok |

## Artefacts (box only, not copied anywhere)

| File | Bytes | sha256 |
|---|---|---|
| omnode aarch64-unknown-linux-musl (phone) | 1,342,856 | `dcf0b2126d47e44a87dcb70ed042e83bd06ed48ebaf511bd4a243349169b0e88` |
| omnode x86_64-unknown-linux-musl (laptop) | 1,581,952 | `3b00a2913f30bb33ad077ebc58ddae8d6596aecf08d2b65a86228060d59f733c` |
| hermes-gateway x86_64-unknown-linux-musl (default features) | 2,177,808 | `49dc2806c1f09d5eeb4f3e92e472ab80b578b36d287c887328c45a1b511fe0e0` |
| hermes-gateway aarch64-unknown-linux-musl (`--no-default-features`) | 1,002,136 | `5714b7362a190971918f4d95c20099765e5144ce2d12e42555d11dce6c1c89e0` |

**Size budget:** hermes-gateway x86_64 musl is **2,177,808 bytes, 80,656 over the 2 MiB budget**.
The base integration-5 0d68f46 built the same way is 1,968,880 (+208,928). Most of that is
hermes-enroll-2 (Ed25519 + enroll store, ~167 KB measured on its own base); this branch adds
about 42 KB (status, outcomes, tunnel, owner CLI). Owner decision D4 in docs/omnode-enroll.md.

## Read-only VPS probe

2026-10-21 17:22:18 +02:00, one read-only ssh command (no writes, no secrets read):
`127.0.0.1:8642` `/health` 200, `/openmind/enroll` 401, `/openmind/node/tunnel` 401 (the live
`hermes-gateway-aa77864` has no enroll code, so the API-key gate answers); one listener on
:8642; `hermes-rs.service` active; no cloudflared ingress rule names port 8642 (grep of the
cloudflared YAML for `8642`, hostname/service lines only). So Hermes has **no public
hostname today**.

## Not tested

* A real Redmi Note 9 Pro (no phone attached to the box): the aarch64 binary ran under
  qemu-user only. Android-specific behaviour (Doze killing the tunnel, Termux curl build,
  CA store) is unverified.
* https through Cloudflare: the curl path ran against plain http on loopback (`--via-curl`).
* A bionic (`aarch64-linux-android`) binary: no NDK on the box, `cargo check` only.
