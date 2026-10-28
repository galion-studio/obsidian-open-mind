<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test record: hades-key-custody (slot 215)

2026-10-21 21:03:14-21:04:19 CEST (+02:00). This record checks the claims in
`docs/security/hades-key-custody.md` against the **real release binary** `hades-gate`, built from
integration-5 `0d68f46` (`cargo-slot build --release -p hades-gate -j 2`, 9.5 s, x86_64-gnu). It
ran on the box in throwaway `/tmp/hkc-real-20261021-210314` (removed afterwards). `serve` ran on
127.0.0.1:20850 (slot band 20850-20859). Two throwaway key pairs (A = 29b4c69062704459,
B = b21bfc3e8d07409f) were made by `hades-gate keygen` and deleted with the folder. No seed, token
or key hex appears below; nonces are masked. Plan bytes: `restart galion-tuner\n`. This was a
manual command session, not a committed script (the row is docs-only, so no test code was added;
see "Follow-up").

Notes on the log: an `exit=` that follows a `| sed` or `| grep` pipe is the pipe's exit, not the
gate's. The gate's own exit codes are in the `chk` lines (`exit=0` approved, `exit=2` refused).

## Results

| # | Claim (doc section) | Result |
|---|---|---|
| K1 | keygen: seed 0600, pub 0644, only the fingerprint printed (§2) | PASS |
| K2 | keygen never overwrites (§2) | PASS (`entity already exists`, exit 1) |
| K3 | gate holds the pub only; `status` fingerprint = keygen fingerprint (§3) | PASS (29b4c69062704459) |
| K4 | seed as `admin_pubkey` refused; pub as `--seed` refused (§3) | PASS (both exit 1) |
| K5 | mint → token file 0600 → check WouldApply, `executed:false` (§4) | PASS |
| K6 | token with TTL 7200 s refused `lifetime` (§4) | PASS |
| K7 | rotation: a running serve keeps the old key until restart; fresh CLI switches at once; forged refusals do not consume; replay refused (§5) | PASS (observed as documented: old-key token **approved** by the running serve after the swap) |
| K8 | disable: refuses a valid token in a running serve + CLI, no restart; enable needs no key; the token survives disable→enable (§6) | PASS (as documented) |
| K8b | disable state not checkable (dir 000, uid 1000) → `disabled` (§6) | PASS |
| K9 | ledger 0600; 0/11 token strings; 0 fingerprints; 0 seed-hex hits in ledger + this log (§5, §7.2) | PASS |
| K10 | no seed file on the gate side (§3) | PASS (0) |
| V1 | VPS read-only: HADES not installed; nerve + cortex state | recorded |

## Raw log

```

[2026-10-21 21:03:14 CEST (+02:00)] K1 keygen on the owner side
{"admin_fingerprint":"29b4c69062704459","ok":true}
exit=0
600 owner/admin.seed
644 owner/admin.pub
pub line tag: ed25519-pub   length=77

[2026-10-21 21:03:14 CEST (+02:00)] K2 keygen never overwrites
hades-gate: owner/admin.seed: entity already exists
exit=1
admin.pub
admin.seed

[2026-10-21 21:03:14 CEST (+02:00)] K3 only the pub goes to the gate host
{"admin_fingerprint":"29b4c69062704459","disabled":false,"mode":"dry-run","service":"hades-gate","ts":"2026-10-21T21:03:14+02:00"}
exit=0

[2026-10-21 21:03:14 CEST (+02:00)] K4 seed handed to the gate as pubkey is refused; pub handed to mint as seed is refused
hades-gate: /tmp/hkc-real-20261021-210314/owner/admin.seed: not a ed25519-pub file
exit=1
hades-gate: owner/admin.pub: not a ed25519-seed file
exit=1

[2026-10-21 21:03:14 CEST (+02:00)] K5 mint (owner) + check (gate)
mint exit=0
600 tok/a1
{"approved":{"action":"heal-apply","exp":"2026-10-21T21:08:14+02:00","iat":"2026-10-21T21:03:14+02:00","nonce":"…","plan_sha256":"b5f8d7595d93d612afcf1832c49830ab9c69506168dc834bf4d6fb8adaa9f669","target":"galion-tuner"},"decision":"WouldApply","dry_run":true,"executed":false,"ts":"2026-10-21T21:03:14+02:00"}
exit=0

[2026-10-21 21:03:14 CEST (+02:00)] K6 TTL ceiling: 7200 s token refused (config default 900, hard 3600)
"code":"lifetime"
exit=0

[2026-10-21 21:03:38 CEST (+02:00)] K7 rotation: serve on 127.0.0.1:20850 with key A
{"admin_fingerprint":"29b4c69062704459","disabled":false,"mode":"dry-run","service":"hades-gate","ts":"2026-10-21T21:03:39+02:00"}
A token via HTTP: 200 "decision":"WouldApply" 

[2026-10-21 21:03:39 CEST (+02:00)] K7 owner makes key B on the owner device and swaps ONLY the pub file on the gate host
{"admin_fingerprint":"b21bfc3e8d07409f","ok":true}
running serve, OLD key A token: 200 "decision":"WouldApply" 
running serve, NEW key B token: 403 "decision":"Refused" "code":"forged"
running serve /health fingerprint: "admin_fingerprint":"29b4c69062704459"
fresh check process, B token: exit=0 "decision":"WouldApply" 
fresh check process, A token: exit=2 "decision":"Refused" "code":"forged"
fresh status: "admin_fingerprint":"b21bfc3e8d07409f"

[2026-10-21 21:03:39 CEST (+02:00)] K7 restart serve -> new key loaded
POST /v1/stop (no token): {"ok":true,"stopping":true}
serve exit=0
restarted serve, A token: 403 "decision":"Refused" "code":"forged"
restarted serve, B token: 200 "decision":"WouldApply" 
restarted serve, replay b1 (was refused forged, not consumed): 200 "decision":"WouldApply" 
replay b3: 403 "decision":"Refused" "code":"replayed"

[2026-10-21 21:03:39 CEST (+02:00)] K8 revocation via disable file (no key, no token, no restart)
{"disabled":true,"ok":true}
644 gate/DISABLED
running serve, valid B token while disabled: 403 "decision":"Refused" "code":"disabled"
fresh check while disabled: exit=2 "decision":"Refused" "code":"disabled"
health: "disabled":true
{"disabled":false,"ok":true}
after enable, same b4 token (not consumed while disabled): 200 "decision":"WouldApply" 

[2026-10-21 21:03:39 CEST (+02:00)] K8b fail closed when the disable switch cannot be checked (dir mode 000 as non-root?)
1000
disable dir unreadable: exit=2 "code":"disabled"
"disabled":true

[2026-10-21 21:04:03 CEST (+02:00)] K9 ledger after the run: mode, events, no token strings, no key fingerprint
600 gate/ledger.jsonl
lines: 15
      7 "event":"consumed"
      8 "event":"refused"
token strings found in ledger: 0 of 11
fingerprints in ledger: A=0 B=0
seed hex hits in ledger+evidence: 0
consumed line keys: "action":"event":"exp":"iat":"nonce":"plan_sha256":"target":"ts":

[2026-10-21 21:04:03 CEST (+02:00)] K10 gate dir holds no seed file
files with seed tag under gate/: 0

[2026-10-21 21:04:19 CEST (+02:00)] V1 VPS read-only (ssh slavik-vps; ls/stat-style existence tests, systemctl list, ss, GET only)
absent /etc/galion/hades ; absent /var/lib/galion/hades ; absent /usr/local/bin/hades-gate ; absent /opt/galion/bin/hades-gate ; hades-gate not on PATH
hades*/gaia* units: gaia-cortex.service loaded active running (ExecStart path=/opt/galion/rust/bin/gaia-cortex-c7f56b8); no hades* unit or unit file; no hades-gate process
listeners: 127.0.0.1:8670 (nerve), 127.0.0.1:8674 (cortex)
nerve GET /state: 200, 37934 B, cycle 10, mode dry-run, score 61 grade D
cortex GET /health: ok, mode dry-run, llm_enabled true, incidents_open 0, escalations_open 0, events_in 19
(disclosure: /state body was saved to /tmp/.s215n on the VPS for counting and removed in the same command)
```

## Live infrastructure during the run

The nerve was observed at 127.0.0.1:8670 `/state` (cycle 10, dry-run, 61/D) and the cortex was
live at 127.0.0.1:8674 `/health` (`gaia-cortex-c7f56b8`, dry-run, llm_enabled true, 0 incidents
open). HADES is not installed on the VPS, so there was no live gate to compare against.
Everything on the VPS was a read: path existence tests, `systemctl list-units`/`show`, `ss -ltn`,
and HTTP GET. The one write was `/tmp/.s215n`, a temp copy of the `/state` body made for counting
and removed in the same command.

## Follow-up (not done: the row is docs-only)

The K1-K10 session could become a `GALION_REAL=1` Rust test (`crates/hades-gate/tests/real_key_custody.rs`,
ports 20850-20859) so these custody properties are checked automatically. This is especially
useful for K7 (rotation window) and K8 (tokens survive disable→enable), since D-KC-6 and D-KC-10
would change those behaviours on purpose.
