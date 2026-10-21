<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: sec-ledger-race (slot 219)

Box: release build of `hades-gate`, ports 127.0.0.1:20890-20895 (slot 219
range). VPS: read-only probes only. Nonces are masked as `<32 hex>`; no token
string is ever printed. Times CEST (+02:00).

## Commands

```
export CARGO_TARGET_DIR=/workspace/swarm/target/sec-ledger-race
GALION_REAL=1 cargo test --release -p hades-gate --test real_ledger_race -- --nocapture --test-threads 1
GALION_REAL=1 HADES_REAL_CLI_PORT=20893 HADES_REDTEAM_PORT=20894 \
  HADES_REAL_LEDGER_FAIL_PORT=20892 HADES_REAL_RACE_PORT=20890 \
  cargo test --release -p hades-gate -- --test-threads 2
```

## Evidence run 2026-10-21 23:02:24-23:02:27 +02:00

```
running 3 tests
start 2026-10-21T23:02:24+02:00
healthy approval: exit Some(0) signal None stdout {"approved":{"action":"heal-apply","exp":"2026-10-21T23:12:24+02:00","iat":"2026-10-21T23:02:24+02:00","nonce":"<32 hex>","plan_sha256":"9cb09bfffdd7f9e876cb3e23b339d0b918f7bfa3a7e776c220ad71bb5dc6202a","target":"galion-tuner"},"audit":"written","decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-21T23:02:24+02:00"} stderr 
no room (fsize = ledger size): exit None signal Some(25) stdout  stderr 
same token, room again: exit Some(0) signal None stdout {"approved":{"action":"heal-apply","exp":"2026-10-21T23:12:24+02:00","iat":"2026-10-21T23:02:24+02:00","nonce":"<32 hex>","plan_sha256":"9cb09bfffdd7f9e876cb3e23b339d0b918f7bfa3a7e776c220ad71bb5dc6202a","target":"galion-tuner"},"audit":"written","decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-21T23:02:24+02:00"} stderr 
torn write (fsize = ledger size + 40): exit None signal Some(25) stdout  stderr 
ledger 770 -> 810 bytes (torn tail 40 bytes)
fresh token after torn write: exit Some(2) signal None stdout {"audit":"failed","audit_error":"ledger corrupt at line 4: failing closed","code":"ledger","decision":"Refused","dry_run":true,"executed":false,"reason":"ledger corrupt at line 4: failing closed","ts":"2026-10-21T23:02:24+02:00"} stderr hades-gate: ledger corrupt at line 4: failing closed; decision Refused (ledger); refusal audit line attempted
torn token again: exit Some(2) signal None stdout {"audit":"failed","audit_error":"ledger corrupt at line 4: failing closed","code":"ledger","decision":"Refused","dry_run":true,"executed":false,"reason":"ledger corrupt at line 4: failing closed","ts":"2026-10-21T23:02:24+02:00"} stderr hades-gate: ledger corrupt at line 4: failing closed; decision Refused (ledger); refusal audit line attempted
status after torn write: exit Some(3) signal None stdout {"admin_fingerprint":"372026c83b76c02c","disabled":false,"ledger":{"audit_failures":0,"error":"ledger corrupt at line 4: failing closed","ok":false,"probe_ok":false},"mode":"dry-run","ok":false,"service":"hades-gate","ts":"2026-10-21T23:02:24+02:00"} stderr hades-gate: ledger corrupt at line 4: failing closed
corrupt line 4: 227 bytes, torn fragment glued to the next record: true
torn token after admin repair: exit Some(0) signal None stdout {"approved":{"action":"heal-apply","exp":"2026-10-21T23:12:24+02:00","iat":"2026-10-21T23:02:24+02:00","nonce":"<32 hex>","plan_sha256":"9cb09bfffdd7f9e876cb3e23b339d0b918f7bfa3a7e776c220ad71bb5dc6202a","target":"galion-tuner"},"audit":"written","decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-21T23:02:24+02:00"} stderr 
ledger lines 7 well-formed, consumed 4; end 2026-10-21T23:02:24+02:00
.start 2026-10-21T23:02:24+02:00 servers on 20890 and 20891
health 20890: HTTP/1.1 200 OK ledger {"audit_failures":0,"audit_failures_total":0,"bytes":58450,"consumed":25,"ok":true,"probe_ok":true}
health 20891: HTTP/1.1 200 OK ledger {"audit_failures":0,"audit_failures_total":0,"bytes":58450,"consumed":25,"ok":true,"probe_ok":true}
mixed race: 25 rounds x (6 cli + 6 http on 2 servers) = 300 attempts; approvals 25 (1 per round), winners {"cli": 4, "serve:20891": 6, "serve:20890": 15}; ledger lines 300 well-formed; elapsed 811.967063ms; end 2026-10-21T23:02:25+02:00
.start 2026-10-21T23:02:25+02:00 bin /workspace/swarm/target/sec-ledger-race/release/hades-gate
process race: 40 rounds x 12 processes = 480 checks; approvals 40 (1 per round); replay refusals 440; ledger lines 480 all well-formed; winner by spawn slot [5, 6, 3, 3, 4, 1, 1, 4, 1, 6, 4, 2]; longest release-to-all-exited 83.786359ms; elapsed 1.911947427s; end 2026-10-21T23:02:27+02:00
.
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.88s
```

Earlier runs 22:59-23:02 +02:00: 1 full file run, 1 run in the full
hades-gate suite, 5 back-to-back runs (`test result: ok. 3 passed` x5).
Full hades-gate suite with all real tests: 54 passed, 0 failed (lib 17,
approval 12, decide 5, journal 5, ledger_fail 5, real_cli 1, real_ledger_fail 1
on 20892, real_ledger_race 3, real_redteam 1 on 20894, redteam 3, simd_free 1).
Dependent crate `gaia`: clippy clean, 35 passed 0 failed.

## VPS, read-only (2026-10-21 23:01 +02:00)

- `systemctl list-units --all 'hades*'`: none; no hades unit file; no
  `hades-gate` on PATH. HADES is not deployed, so there is no live ledger to
  compare against.
- Nerve `GET 127.0.0.1:8670/state` (ts 22:57:57 +02:00): mode live, system
  76 / C, degraded, 2 open incidents; cortex sensors listed: listeners,
  scanners, edge, components, ddos (no `hades` sensor reporting yet).
  `gaia-cortex.service` and `galion-nerve.service` active.
- `journalctl --since -24h | grep -c -e 'ledger unavailable' -e 'ledger corrupt'`: 0.
