<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: hades-redteam (S3.1)

Run 2026-10-20 22:57:51 CEST +02:00 on the box (not the VPS). No VPS probe: HADES gate is not deployed on the
VPS (nothing to compare against), and the planning phase forbids VPS writes.

Test: `crates/hades-gate/tests/real_redteam.rs`. It runs `hades-gate keygen` (throwaway key in a temp dir),
starts the **release** binary with `hades-gate serve` on 127.0.0.1, and for each of the 82 corpus attacks runs
`hades-gate mint` to sign a valid token for that exact plan, then `POST /v1/approve`. Then the 9 legitimate
plans get the same treatment and must get `WouldApply`. Then `POST /v1/stop`, kill, temp dir removed. Token
strings are never printed.

## Commands

```
export CARGO_TARGET_DIR=/workspace/swarm/target/hades-redteam CARGO_INCREMENTAL=0
# before: ccaab42 release binary, port 19381
GALION_REAL=1 HADES_GATE_BIN=.../base/release/hades-gate HADES_REDTEAM_PORT=19381 \
  nice -n 10 cargo test -q --release -j 2 -p hades-gate --test real_redteam -- --nocapture
# after: this branch's release binary, port 19380
GALION_REAL=1 nice -n 10 cargo test -q --release -j 2 -p hades-gate --test real_redteam -- --nocapture
ss -ltn | grep -E ':1938[0-9]'     # -> nothing: no listeners left
```

## Output

Before (ccaab42), test fails as it should:

```
REAL shell: 16 WouldApply / 25
REAL file: 12 WouldApply / 19
REAL access: 6 WouldApply / 14
REAL allowverb: 9 WouldApply / 10
REAL unicode: 0 WouldApply / 6
REAL traversal: 0 WouldApply / 8
REAL TOTAL WouldApply: 43 / 82
REAL legit not approved: 0 / 9
[shell] ${IFS} instead of space   HTTP/1.1 200 OK WOULDAPPLY
[file] find -delete               HTTP/1.1 200 OK WOULDAPPLY
[access] systemctl mask           HTTP/1.1 200 OK WOULDAPPLY
[traversal] Cyrillic target       HTTP/1.1 403 Forbidden {"code":"malformed",...,"reason":"malformed token: target name"}
```

After (this branch), `test result: ok. 1 passed`:

```
REAL shell: 0 WouldApply / 25
REAL file: 0 WouldApply / 19
REAL access: 0 WouldApply / 14
REAL allowverb: 0 WouldApply / 10
REAL unicode: 0 WouldApply / 6
REAL traversal: 0 WouldApply / 8
REAL TOTAL WouldApply: 0 / 82
REAL legit not approved: 0 / 9
[shell] ${IFS} instead of space   HTTP/1.1 403 Forbidden {"code":"admin-safety","decision":"Refused","dry_run":true,"executed":false,"reason":"admin-safety: destructive command refused (rm)","ts":"2026-10-20T22:58:03+02:00"}
[file] find -delete               HTTP/1.1 403 Forbidden {"code":"admin-safety",...,"reason":"admin-safety: destructive command refused (find)"}
[access] systemctl mask           HTTP/1.1 403 Forbidden {"code":"admin-safety",...,"reason":"admin-safety: admin account/access change refused (service manager)"}
[shell] sh -c payload             HTTP/1.1 403 Forbidden {"code":"admin-safety",...,"reason":"admin-safety: plan cannot be reviewed (shell)"}
```

The 43 (not 48) before: in the full `Approver` path `token::valid_target` already refused the 5 bad target
names before `safety::review`. Every response carried `"executed":false`.
