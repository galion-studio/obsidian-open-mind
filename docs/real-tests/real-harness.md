<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: galion-realtest (swarm/real-harness, slot 14)

All times are Europe/Zagreb (CEST, +02:00). Box: `Linux 6.12.94+ x86_64`. Base commit 13a296c.
No secret values appear here. The pairing token, the fixture memory key and the relay sentinels exist only in memory or in the throwaway fixture dir, and the key file is deleted after the run.

## 1. Box stack at 13a296c (main)
Command (2026-10-20T19:20:49+02:00):
```
LIVE=1 scripts/real/real-harness.sh
# = nice -n 10 cargo build --release --locked -j 4 -p galion-realtest -p galion-boot -p galion-one \
#     -p openmind-memory -p tuner -p galion-desk -p link-watch -p guardian-tick -p persona-plex-relay
#   target/release/galion-realtest stack --bin-dir target/release --out target/realtest/stack
#   target/release/galion-realtest live --out target/realtest/live
```
Ports 127.0.0.1:19500-19551. Everything ran under `galion-boot supervise` in one process group. Boot from supervisor start to all six HTTP services healthy took about 0.3 s. Whole run: about 11 s.

Checks: 77 total, 66 pass, 0 unexpected FAIL, 10 known-fail, 1 skipped.

| Scenario | Service | Verdict |
|---|---|---|
| stack-boot | galion-boot | PASS (8/8) |
| one-pairing | galion-one | PASS (11/11): wrong code refused; right code issued an HttpOnly, SameSite=Strict cookie; `/me` you:true and analytics 200 with that cookie; right code during lockout returned `{"error":"later"}`; store holds hashes only |
| one-fail-closed-missing-store | galion-one | **FAIL (5 of 7; listed as known)**. `/me` returns `{"you":true}` and `/session`, `/api/v1/analytics` and `/files` return 200 without a cookie |
| one-fail-closed-empty-store | galion-one | **FAIL (5 of 7; listed as known)**, same fail-open |
| memory-save-search | openmind-memory | PASS (9/9): `/capture` gave `{"l0_recorded":2}`, `/search/conversations` total 2, the other session saw 0, missing/wrong bearer got 401 |
| nerve-ingest | openmind-nerve | SKIP: no nerve binary exists at 13a296c |
| tuner | tuner | PASS (7/7) |
| link-watch | link-watch | PASS (5/5): model `galion-harness ctx 4096`; Hermes down detected 2.75 s after the stub went 503; event logged; recovered |
| desk | galion-desk | PASS (7/7) |
| voice-relay-contract | persona-plex-relay | PASS (5/5): the stamp is `2026-10-20T19:20:54+02:00`; a 29-line plan with no secret printed; exit codes 2 and 1 |
| guardian-tick-heal | guardian-tick | PASS (6/6): `host_ok=false probes=4 failed=1 / suggest: tuner down: operator may restart tuner`; tuner healthy again 3957 ms after SIGKILL, pid 1049817 -> 1050738; then `host_ok=true` |
| heal-request | galion-boot | PASS (3/3): the restart file restarted galion-desk only (pid changed in 1.0 s) |
| teardown | galion-realtest | PASS: process group signalled; ports 19500-19551 all free |

Stability: I ran it 3 more times in a row (`flake1..3`), and every run exited 0. `GALION_REAL=1 cargo test -p galion-realtest --test real_stack` passed in 11.4 s.

## 2. Cross-check: the same harness against the swarm/one-probe fix
`galion-one` was built from `swarm/one-probe` b44cd4f (exported with `git archive`, built in my own target dir). The other 7 binaries came from 13a296c. I ran `galion-realtest stack --known-failures /dev/null` (2026-10-20T19:17 CEST): **all 12 scenarios PASS, exit 0**. So the harness tells the fail-open build and the fixed build apart. The 10 known-failure lines can be removed once that fix is on main.

## 3. Live probe (READ-ONLY), 2026-10-20T19:21+02:00
`galion-realtest live`: one `ssh slavik-vps sh -s` batch of GET-only `curl`, `systemctl is-active` and `ss -ltnH`, plus public GETs from the box. Checks: 43 total, 41 pass, 2 known-fail.

| Service | Contract | Result |
|---|---|---|
| galion-one :8091 | `/health` = ok; `/me` you:false; `/session` 401 unpaired | PASS (live store has a code, so it is closed) |
| galion-one mouth :8002 | `/health` 200 ok:true | **FAIL**: 503 `{"ok":false,"status":"loading",...,"model":"chatterbox"}` (needs a GPU; listed as known) |
| openmind-memory :8420 | status ok, implementation openmind-memory | PASS |
| hermes-rs :8642 | status ok | PASS (`implementation":"hermes-rs"`) |
| galion-desk :8646 | `{"ok":true}` | PASS |
| tuner :8650 | `{"ok":true}` | PASS |
| galion-health :8660 | `/status.json` ok:true | PASS |
| galion-nerve :8670 | `/healthz` ok, dashboard | PASS |
| tanga-serve :8671 | `/tanga/` html | PASS |
| console :8080 | `/` 200/302; `/health` 404 until switched (next.md) | PASS (302 / 404) |
| cloudflared :20241 | `/ready` 200 (body not recorded) | PASS |
| units | 10 units active | 9 active; **link-watch.service inactive/disabled** since 2026-10-18 19:42 CEST (listed as known) |
| ports | 11 listeners | all 11 listening |
| public one.galion.app | `/health` ok, `/` html, `/me` you:false, `/session` 401, `/api/v1/analytics` 401, `/tanga/` html | PASS (6/6) |

## 4. Findings for the owner
1. **galion-one fails open on main** when its device store is missing or has no `codeHash` (`castle()` false means `token_ok()` true): unpaired clients get `/session`, analytics and files. Live is not affected today because the live store has a code. The fix is on swarm/one-probe.
2. galion-one's `/api/v1/analytics` `at` field is **UTC** (`"at":"17:20:49"` while Zagreb time was 19:20:49), even with `TZ=Europe/Zagreb`. That breaks the timezone rule (main.md section 10).
3. The One mouth (TTS) on the VPS is 503 "loading" (no GPU).
4. link-watch.service on the VPS has been disabled since Oct 18. The Rust link-watch passes on the box, but its cutover needs approval.
5. No nerve ingest binary exists at 13a296c. The live `galion-nerve` on :8670 answers `/healthz`, but its source is not on main.

## 5. Honesty notes
- Before the harness existed, one manual exploratory probe on the VPS used `curl -o /tmp/.rh-probe-body` and removed that file right after (one tiny temp file in /tmp). The harness itself writes nothing on the VPS; its script is pinned GET-only by a unit test.
- The live checks show 0 ms per check because they are judged after one batch. The batch time is in the report header.
