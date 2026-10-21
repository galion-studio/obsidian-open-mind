<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: gaia-live (first end-to-end Gaia path, box only)

Run: 2026-10-20 19:16:46 to 19:16:53 CEST (+02:00), Europe/Zagreb, on the box (swarm slot 03, ports 19030-19039).
Branch `swarm/gaia-live`. Command: `scripts/real/gaia-live.sh /tmp/gaia-live-real-final`, exit 0.
Also: `GALION_REAL=1 GAIA_NERVE_BIN=$CARGO_TARGET_DIR/release/openmind-nerve cargo test -p gaia --test real_gaia_live` passed (1 test, 1.6 s; it uses 19035 for the nerve and 19036 for the dummy).

**Scope.** Everything ran on the box. The only processes were ones this test started: the real `openmind-nerve serve` on 127.0.0.1:19031, `gaia-dummy` on 127.0.0.1:19032, and the restarted dummies. Nothing contacted the VPS, systemd or any port outside 19030-19039. I did no VPS probes, because the task forbids targeting VPS services. Port 8420 shows up only as a negative case, and Gaia refused it before sending any packet. After teardown, `pgrep` found no leftover gaia-dummy or openmind-nerve processes, and `ss -ltn` showed 19030-19039 free.

## The path
```
gaia-dummy down
  -> gaia heal-tick: read-only GET /health (DEMETER guardian-tick probe)
  -> POST nerve /event  kind=service.probe severity=warn   (openmind-nerve, JSONL, Zagreb ts)
  -> GAIA core reads the event BACK from the nerve (GET /events) and signals it on its towers
  -> DEMETER: healer heal-dry (Plan) + guardian-tick health cycle (Run)
  -> HADES: heal-apply Plan (dry-run), then heal-apply Execute
  -> approval check: approval file (alpha, action=heal-apply, service, token) vs the run's trusted
     TEST token (constant-time, 128-bit random, mode 600, never printed: only an fnv fingerprint),
     Alpha must be registered; GAIA core binds the approval to the directive id + target
  -> HADES releases the plan (outcome done, dry_run false); HADES itself executes nothing
  -> healer apply: restart cap check (3 per service per rolling hour, on-disk ledger),
     spawn the recorded start argv detached, wait for /health
  -> POST nerve /event  kind=gaia.heal.applied | gaia.heal.refused | gaia.heal.capped | gaia.heal.failed
```
Guards (crates/gaia-live/src/heal.rs). Any failure is a refusal that is recorded on the nerve:
- Target must be 127.0.0.1, its port must be inside `--allow-ports`, and that range must sit inside 19000-19999. Live stack ports (8002, 8080, 8091, 8420, 8642, 8646, 8650, 8670) are always refused, even when an operator passes `--allow-ports 1-65535`.
- Start program must be an absolute path to an existing file, and it must not be a service manager, shell, `env`, sudo/su, ssh/scp/rsync, a killer, `rm` or `dd`, or python. Its arguments must name the target port.
- Gaia never calls `systemctl`, never sends a signal and never kills anything. The kills in this test are the script's own `kill <pid>` of the dummy it started.

## Output of the run (verbatim; the token never appears)
```

[2026-10-20 19:16:46 CEST (+02:00)] build (release, nice -n 10, -j 4, CARGO_TARGET_DIR=/workspace/swarm/target/gaia-live)
684720 /workspace/swarm/target/gaia-live/release/gaia
325472 /workspace/swarm/target/gaia-live/release/gaia-dummy
423272 /workspace/swarm/target/gaia-live/release/openmind-nerve

[2026-10-20 19:16:51 CEST (+02:00)] per-run TEST approval token (random, 128 bits, mode 600, never printed)
-rw------- /tmp/gaia-live-real-final/approval
-rw------- /tmp/gaia-live-real-final/trust.token

[2026-10-20 19:16:51 CEST (+02:00)] start the real openmind-nerve on 127.0.0.1:19031

[2026-10-20 19:16:51 CEST (+02:00)] start gaia-dummy on 127.0.0.1:19032
nerve  GET /health: {"appended":0,"service":"openmind-nerve","status":"ok","ts":"2026-10-20T19:16:51+02:00","version":"0.1.0"}
dummy  GET /health: {"status":"ok","service":"gaia-dummy","pid":989719,"started":"2026-10-20T19:16:51+02:00"}

[2026-10-20 19:16:51 CEST (+02:00)] 1. dummy up: gaia heal-tick sees it healthy
1-healthy      exit=0 outcome=healthy detail=service is up; nothing to do

[2026-10-20 19:16:51 CEST (+02:00)] 2. scope guard: a live stack port (8420) is refused before any probe
2-live-port    exit=4 outcome=refused detail=port 8420 is outside the allowed test range 19030-19039

[2026-10-20 19:16:52 CEST (+02:00)] 3. kill the dummy (pid 989719, our own child)
dummy GET /health now: connection refused

[2026-10-20 19:16:52 CEST (+02:00)] 4. detected + planned, but no approval: refused
4-no-approval  exit=4 outcome=refused detail=no approval file given

[2026-10-20 19:16:52 CEST (+02:00)] 5. wrong token: refused
5-wrong-token  exit=4 outcome=refused detail=approval token fnv:1372d4dd does not match the trusted test token

[2026-10-20 19:16:52 CEST (+02:00)] 6.1 approved with the test token: restart 1 of 3
6.1-restart    exit=0 outcome=restarted detail=restart 1/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":989787,"started":"2026-10-20T19:16:52+02:00"}

[2026-10-20 19:16:52 CEST (+02:00)] kill the restarted dummy (pid 989787)

[2026-10-20 19:16:52 CEST (+02:00)] 6.2 approved with the test token: restart 2 of 3
6.2-restart    exit=0 outcome=restarted detail=restart 2/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":990119,"started":"2026-10-20T19:16:52+02:00"}

[2026-10-20 19:16:52 CEST (+02:00)] kill the restarted dummy (pid 990119)

[2026-10-20 19:16:53 CEST (+02:00)] 6.3 approved with the test token: restart 3 of 3
6.3-restart    exit=0 outcome=restarted detail=restart 3/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":990414,"started":"2026-10-20T19:16:53+02:00"}

[2026-10-20 19:16:53 CEST (+02:00)] kill the restarted dummy (pid 990414)

[2026-10-20 19:16:53 CEST (+02:00)] 7. fourth restart within the hour: capped
7-capped       exit=5 outcome=capped detail=restart cap reached: 3 restarts of gaia-dummy in the last hour; next allowed in 3599 s

[2026-10-20 19:16:53 CEST (+02:00)] the step trail of restart 1 (GAIA core, DEMETER, HADES, approval, apply)
{"step":"scope","at":"2026-10-20T19:16:52+02:00","d":{"ok":true,"allowed":"127.0.0.1:19030-19039"}}
{"step":"probe","at":"2026-10-20T19:16:52+02:00","d":{"state":"down","notes":["GET :19032/health -> connect: connection refused"]}}
{"step":"nerve-in","at":"2026-10-20T19:16:52+02:00","d":{"data":{"detail":"connect: connection refused","notes":["GET :19032/health -> connect: connection refused"],"port":19032,"probe_id":"gaia-dummy-1792516612-989785","service":"gaia-dummy","state":"down"},"kind":"service.probe","severity":"warn","source":"gaia-live","ts":"2026-10-20T19:16:52+02:00"}}
{"step":"demeter","at":"2026-10-20T19:16:52+02:00","d":{"live_suggestion":"restart gaia-dummy on 127.0.0.1:19032 with its recorded start command (connect: connection refused)","healer":"heal-dry: 0 suggestion(s); apply is a HADES plan","guardian_tick":"health cycle 1: mode full"}}
{"step":"hades-plan","at":"2026-10-20T19:16:52+02:00","d":{"outcome":"dry-run","summary":"dry-run plan","dry_run":true,"plan":["take the heal-dry findings from DEMETER","apply at most 1 allow-listed fixes","append every action to actions.jsonl","record the plan in lineage"]}}
{"step":"approval","at":"2026-10-20T19:16:52+02:00","d":{"alpha":"owner","token_fp":"fnv:53771782","check":"ok","released":true,"hades":"approved by an Alpha: plan released to the operator; HADES itself executes nothing"}}
{"step":"cap","at":"2026-10-20T19:16:52+02:00","d":{"ok":true,"restarts_last_hour_before":0,"cap":3}}
{"step":"apply","at":"2026-10-20T19:16:52+02:00","d":{"action":"restart","pid":989787,"health_verified":true,"health_tries":2,"restarts_last_hour":1,"cap":3,"spawn_error":null,"log":"/tmp/gaia-live-real-final/state/gaia-dummy.log"}}

[2026-10-20 19:16:53 CEST (+02:00)] nerve GET /events?n=100 (kind, severity, ts, short data)
2026-10-20T19:16:51+02:00	info	service.probe	{"state":"up"}
2026-10-20T19:16:51+02:00	warn	gaia.heal.refused	{"stage":"scope","reason":"port 8420 is outside the allowed test range 19030-19039"}
2026-10-20T19:16:52+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:52+02:00	warn	gaia.heal.refused	{"stage":"approval","reason":"no approval file given"}
2026-10-20T19:16:52+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:52+02:00	warn	gaia.heal.refused	{"stage":"approval","reason":"approval token fnv:1372d4dd does not match the trusted test token"}
2026-10-20T19:16:52+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:52+02:00	info	gaia.heal.applied	{"restarts_last_hour":1,"pid":989787,"health_verified":true}
2026-10-20T19:16:52+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:52+02:00	info	gaia.heal.applied	{"restarts_last_hour":2,"pid":990119,"health_verified":true}
2026-10-20T19:16:53+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:53+02:00	info	gaia.heal.applied	{"restarts_last_hour":3,"pid":990414,"health_verified":true}
2026-10-20T19:16:53+02:00	warn	service.probe	{"state":"down"}
2026-10-20T19:16:53+02:00	error	gaia.heal.capped	{"restarts_last_hour":3,"retry_after_s":3599}
nerve JSONL lines on disk: 14
restart ledger: {"service":"gaia-dummy","ts":"2026-10-20T19:16:52+02:00","pid":989787} {"service":"gaia-dummy","ts":"2026-10-20T19:16:52+02:00","pid":990119} {"service":"gaia-dummy","ts":"2026-10-20T19:16:53+02:00","pid":990414} 

[2026-10-20 19:16:53 CEST (+02:00)] token leak check (trusted token must not appear in any output)
ok: token not present in tick output, nerve log or state

[2026-10-20 19:16:53 CEST (+02:00)] teardown
port 19031 closed
port 19032 closed

[2026-10-20 19:16:53 CEST (+02:00)] done; evidence in /tmp/gaia-live-real-final
```

Two nerve JSONL lines exactly as they are on disk (`nerve.jsonl`):
```
{"ts":"2026-10-20T19:16:52+02:00","source":"gaia-live","kind":"gaia.heal.applied","severity":"info","data":{"action":"restart","approved_by":"owner","cap":3,"hades_directive":4,"health_verified":true,"pid":989787,"port":19032,"probe_id":"gaia-dummy-1792516612-989785","restarts_last_hour":1,"service":"gaia-dummy","spawn_error":null}}
{"ts":"2026-10-20T19:16:53+02:00","source":"gaia-live","kind":"gaia.heal.capped","severity":"error","data":{"cap":3,"port":19032,"probe_id":"gaia-dummy-1792516613-990765","restarts_last_hour":3,"retry_after_s":3599,"service":"gaia-dummy"}}
```

## What it shows
| Requirement | Evidence |
|---|---|
| Real nerve binary on our port | `openmind-nerve serve` on 127.0.0.1:19031, GET /health `status ok`; 14 events, 14 JSONL lines, all `+02:00` |
| Gaia detects the kill | `service.probe` `state down`, `connect: connection refused` |
| Nerve event drives GAIA core | step `nerve-in` is the record read back from the nerve by `probe_id` (GAIA refuses to continue when the nerve does not return it) |
| DEMETER / HADES plan | steps `demeter` (live suggestion, heal-dry, guardian-tick cycle) and `hades-plan` (dry-run, 4 steps) |
| Approval token check | no approval: refused; wrong token: refused (fingerprint only); correct test token: `check ok`, HADES `released true` |
| Restart applied | 3 restarts with new pids 989787, 990119, 990414, each with health verified |
| 3 per hour cap | 4th tick: exit 5, `gaia.heal.capped`, `retry_after_s 3599` |
| Nerve records the result | `gaia.heal.applied` x3, `gaia.heal.refused` x3, `gaia.heal.capped` x1 |
| VPS untouchable | live port 8420 refused at `scope` before any probe |

## Gates on this branch (2026-10-20 ~19:15 CEST)
- `cargo fmt --check` ok. `cargo clippy --all-targets --workspace -- -D warnings` ok. `cargo test --workspace`: 593 passed, 0 failed.
- `scripts/no-python.sh` ok. `scripts/spdx-scan.sh` missing 0. `repo-check --root .` PASS (0 errors, 45 warnings, which were already there except two `SystemTime::now()` notes in gaia-live that only feed the epoch ledger and zagreb-time).
- Release sizes (opt-level z, lto, strip): x86_64 musl: gaia 811,808 B, gaia-dummy 438,960 B, openmind-nerve 516,784 B. aarch64 musl: 702,696 / 386,320 / 449,752 B. aarch64 musl was linked with rust-lld through `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`, because the box's system `ld` rejects `--fix-cortex-a53-843419`; nothing about this was committed. `cargo check` passes for aarch64-linux-android and armv7-unknown-linux-musleabihf.

## Limits (honest)
- This is a **test-service** path. It has no unit for live services, and wiring it to a live service needs the owner's approval (docs/cutover-approval.md).
- The approval token is a per-run random **test** token in a mode-600 file. It is not the hard-coded `"approved-test"` of hades-gate, and hades-gate is not used here. A real Alpha credential (a signature or key) is still open work.
- DEMETER's simulated heal-dry reads GAIA's model vitals, so it reports 0 suggestions. The live suggestion comes from the probe and is passed as the HADES payload.
- One tick is one run of `gaia heal-tick`. There is no daemon or timer loop yet. A guardian-tick timer would call it.
- The restart cap ledger is local to `--state DIR`, so a different state dir gets a fresh budget.
