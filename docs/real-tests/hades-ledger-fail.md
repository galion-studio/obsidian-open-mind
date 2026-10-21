<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: hades-ledger-fail (slot 125, finding 4 of cortex-s2-d)

- Branch `swarm/hades-ledger-fail`, base `swarm/hades-redteam` e6ceca7. Box only, ports 19950-19951 (slot 125).
  hades-gate is not deployed on the VPS, so there is no live gate to compare; read-only VPS evidence below.
- Test: `crates/hades-gate/tests/real_ledger_fail.rs` (release binary, `GALION_REAL=1`):

```
CARGO_TARGET_DIR=/workspace/swarm/target/hades-ledger-fail GALION_REAL=1 HADES_REDTEAM_PORT=19951 \
  cargo-slot test --release -j 2 -p hades-gate --test real_ledger_fail --test real_redteam --test real_cli -- --nocapture
```

Full disk = ledger symlinked to `/dev/full` (open works, every write ENOSPC). Lost permission = `chmod 0400` on the
ledger while `serve` runs (test user `box`, not root).

## Before (e6ceca7 release binary), 2026-10-21 04:36:47 +02:00

```
== OLD: ledger -> /dev/full (full disk), check dry-run
exit 0 stderr=[]
stdout {"decision":"DryRun","dry_run":true,"executed":false,"reason":"no token: plan only","ts":"2026-10-21T04:36:47+02:00"}
== OLD: forged token
exit 2 stderr=[] code="code":"malformed"
== OLD: status
exit 0 stderr=[] stdout {"admin_fingerprint":"19edf6f37d4b3edb","disabled":false,"mode":"dry-run","service":"hades-gate","ts":"2026-10-21T04:36:47+02:00"}
```

Nothing written, nothing said, status healthy.

## After (this branch), same commands, 2026-10-21 04:36:47 +02:00

```
== NEW: ledger -> /dev/full (full disk), check dry-run
exit 3 stderr=[hades-gate: ledger unavailable (no storage space): failing closed; decision DryRun (dry-run); audit line not written]
stdout {"audit":"failed","audit_error":"ledger unavailable (no storage space): failing closed","decision":"DryRun",...}
== NEW: forged token
exit 2 stderr=[hades-gate: ledger unavailable (no storage space): failing closed; decision Refused (malformed); audit line not written] code="code":"malformed"
== NEW: status
exit 3 stderr=[hades-gate: ledger unavailable (not a regular file): failing closed] stdout {...,"ledger":{"audit_failures":0,"error":"ledger unavailable (not a regular file): failing closed","ok":false,"probe_ok":false},"mode":"dry-run","ok":false,...}
```

## Rust real test output (2026-10-21 04:36:24 +02:00, verbatim, fingerprints are throwaway keys)

```
cli healthy dry-run: exit Some(0) stdout {"audit":"written","decision":"DryRun",...}
cli healthy status: exit Some(0) stdout {...,"ledger":{"audit_failures":0,"bytes":140,"consumed":0,"ok":true,"probe_ok":true},...,"ok":true,...}
cli full-disk dry-run: exit Some(3) stderr hades-gate: ledger unavailable (no storage space): failing closed; decision DryRun (dry-run); audit line not written
cli full-disk token: exit Some(2) stderr hades-gate: ledger unavailable (not a regular file): failing closed; decision Refused (ledger); refusal audit line attempted
cli full-disk status: exit Some(3) stdout {...,"ledger":{..,"error":"ledger unavailable (not a regular file): failing closed","ok":false,"probe_ok":false},...}
serve health (ok): HTTP/1.1 200 OK {"audit_failures":0,"audit_failures_total":0,"bytes":140,"consumed":0,"ok":true,"probe_ok":true}
serve ro malformed token: HTTP/1.1 403 Forbidden {"audit":"failed","audit_error":"ledger unavailable (permission denied): failing closed","code":"malformed",...}
serve ro dry-run: HTTP/1.1 503 Service Unavailable {"audit":"failed","audit_error":"ledger unavailable (permission denied): failing closed","decision":"DryRun",...}
serve ro valid token: HTTP/1.1 403 Forbidden code "ledger"
serve health (ro): HTTP/1.1 503 Service Unavailable {"audit_failures":3,"audit_failures_total":3,"error":"ledger unavailable (permission denied): failing closed","ok":false,"probe_ok":false}
serve full dry-run: HTTP/1.1 503 Service Unavailable audit_error "ledger unavailable (no storage space): failing closed"
serve health (after full, probe ok): HTTP/1.1 503 Service Unavailable {"audit_failures":4,"audit_failures_total":4,"bytes":280,"consumed":0,"ok":false,"probe_ok":true}
serve health (recovered): HTTP/1.1 200 OK {"audit_failures":0,"audit_failures_total":4,"bytes":280,"consumed":0,"ok":true,"probe_ok":true}
---- serve stderr (journal) ----
hades-gate: serving on 127.0.0.1:19950 (dry-run)
hades-gate: ledger unavailable (permission denied): failing closed; decision Refused (malformed); audit line not written
hades-gate: ledger unavailable (permission denied): failing closed; decision DryRun (dry-run); audit line not written
hades-gate: ledger unavailable (permission denied): failing closed; decision Refused (ledger); refusal audit line attempted
hades-gate: ledger unavailable (permission denied): failing closed; /health unhealthy
hades-gate: ledger unavailable (no storage space): failing closed; decision DryRun (dry-run); audit line not written
hades-gate: ledger ok again (audit failures so far: 4)
hades-gate: stopped by admin request
----
sensor-matching journal lines: 5; elapsed 111.004909ms
ledger lines kept: 2
test real_ledger_write_failure_is_detectable ... ok
```

Checked by the test: exit codes, HTTP codes, `audit` fields, 5 journal lines matching the cortex `hades` sensor's
`ledger unavailable` pattern, recovery line, and that the journal holds no target, plan or token text. Server
killed by `Drop` on failure, stopped by `POST /v1/stop` on success; temp dir removed.

## Regression (2026-10-21 04:38:39-04:38:47 +02:00)

- `real_redteam` over HTTP on :19951: `REAL TOTAL WouldApply: 0 / 82` (unchanged from e6ceca7).
- `real_cli` (existing test, its fixed port 19021): ok.

## Gates

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 504 passed, 0 failed (hades-gate: lib 17,
  approval 12, decide 5, journal 5, ledger_fail 5 new, redteam 3, simd_free 1, real_* gated).
- `cargo check -p hades-gate`: x86_64/aarch64 musl, aarch64-linux-android, armv7-unknown-linux-musleabihf ok.
  Release musl: x86_64 623312 bytes, aarch64 538064 bytes (`linker=rust-lld`).
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` scanned 192, missing 0.

## Re-run by the resumed slot-125 agent, 2026-10-21 04:46:45-04:47:19 +02:00

All three real tests in the slot range (`real_cli` now takes `HADES_REAL_CLI_PORT`, default 19021 unchanged):

```
GALION_REAL=1 HADES_REDTEAM_PORT=19951 HADES_REAL_CLI_PORT=19952 cargo-slot test --release -j 2 -p hades-gate \
  --test real_ledger_fail --test real_redteam --test real_cli -- --nocapture --test-threads=1
test real_cli_and_http_round_trip ... ok
sensor-matching journal lines: 5; elapsed 113.691203ms
ledger lines kept: 2
test real_ledger_write_failure_is_detectable ... ok
REAL TOTAL WouldApply: 0 / 82
test real_redteam_over_http ... ok
```

Gates re-run 04:44-04:46 +02:00: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok;
`cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 121 result lines, 504 passed, 0 failed.

## Real server (VPS), read-only, 2026-10-21 04:47:29 +02:00

- `systemctl list-units --all | grep -iE 'hades|cortex|nerve'`: only `galion-nerve.service` (:8670) and
  `galion-nerve-shadow.service` (:8672) - no hades-gate unit, no cortex unit (cortex not live yet, so the nerve is the observer).
- `GET http://127.0.0.1:8670/state`: HTTP 200, 13139 B, 15 components (cloudflared, console, gaia, galion-desk,
  galion-health, galion-nerve, galion-one, galion-one-mouth, hermes-rs, link-watch-rs, lxc-host, openmind-memory, tuner,
  tuner-quick, voice-worker); Gaia function `HADES` has `"components":[]`.
- `journalctl --since -24h | grep -c 'ledger unavailable'` = 0; `grep -c hades-gate` = 0.
- So the ledger-failure signal has no live emitter yet: it starts reaching the nerve/cortex `hades` sensor
  (`sensors/hades.rs` on cortex-s2-d f3468ce, `Class::LedgerFail` on `ledger unavailable`/`ledger corrupt`) only once
  hades-gate is deployed as a `hades*` unit. Deploying it is an owner decision; nothing was written on the VPS.
