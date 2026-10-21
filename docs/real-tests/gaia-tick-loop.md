<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: gaia-tick-loop (Gaia S3.1, box only)

Wave-4 slot 160, branch `swarm/gaia-tick-loop` on integration-3 625eaf0. All times are Europe/Zagreb, CEST (+02:00), 2026-10-21.
Design and owner decisions: docs/gaia-heal-loop.md.

## Scope and ports
- Everything ran **on the box**. Release binaries: `gaia`, `gaia-dummy`, `openmind-nerve`, all built from this branch.
- Real nerve: `openmind-nerve serve` on 127.0.0.1:20300 (slot 160 range 20300-20309).
- Dummies: 127.0.0.1:19601 (hang), 19602 (flap), 19603 (crash), with `--allow-ports 19600-19609`. **Deviation:** they are not on 20301-20309, because the heal apply path only ever touches 19000-19999 (`gaia_live::heal::TEST_PORTS`). That is a safety guard, and this slot did not widen it. Before every run, 19600-19609 was free (`ss -ltn`) and unused in CALL-RECORD.
- No VPS install, no unit or timer, no VPS write. VPS access was read-only GETs of the live nerve `/state` and a unit/timer listing (below).
- The test kills only processes it started: its own children, plus gaia-dummy processes whose cmdline names 19601-19603.

## The real test
`crates/gaia/tests/real_gaia_loop.rs`, gated by `GALION_REAL=1`. The Rust test builds, starts, probes and tears down by itself; there is no script.
```
GALION_REAL=1 GAIA_NERVE_BIN=$CARGO_TARGET_DIR/release/openmind-nerve \
  cargo-slot test --release -j 2 -p gaia --test real_gaia_loop -- --nocapture
```
Flow:
1. Start the nerve and three fault dummies.
2. Pre-mint 5 Ed25519 hades-gate tokens per service (TTL 600 s, mode 0600; the seed stays in the temp dir).
3. Start three `gaia heal-tick --loop` processes at once with interval 300 ms, jitter 200 ms, wait 2.5 s, one shared hades config, and their own `--state` and `--token-dir`.
4. Poll the nerve's JSONL log until each fault shows its documented outcome.
5. Check for zombies, stop all three loops with the stop file, and check the summaries, tokens and ledgers.
6. Run a one-shot tick and a fourth loop, each on a brand-new `--state`. Both must be capped.
7. Check nerve counts, Zagreb stamps and that no token text appears.
8. Tear down and prove no process is left and no port is open.

## Results: 6 passing runs
Runs 1-3 (07:11, 07:18, 07:19) were on the code before the scope fail-fast. Runs 4-6 (07:21, 07:22, 07:23) were on the final code. All 6 passed, each in 18.9-19.0 s.

| Run | settle | hang ticks / outcomes | flap ticks / outcomes | crash ticks / outcomes | stop after STOP | nerve events |
|---|---|---|---|---|---|---|
| 1 07:11 | 16.51 s | 6 / failed 3, capped 3 | 43 / restarted 3, healthy 9, capped 31 | 45 / restarted 1, healthy 44 | 100 ms | 158 |
| 2 07:18 | 16.52 s | 6 / failed 3, capped 3 | 45 / restarted 3, healthy 10, capped 32 | 45 / restarted 1, healthy 44 | 101 ms | 161 |
| 3 07:19 | 16.51 s | 6 / failed 3, capped 3 | 43 / restarted 3, healthy 10, capped 30 | 44 / restarted 1, healthy 43 | 100 ms | 156 |
| 4 07:21 | 16.51 s | 6 / failed 3, capped 3 | 43 / restarted 3, healthy 9, capped 31 | 44 / restarted 1, healthy 43 | 100 ms | 157 |
| 5 07:22 | 16.52 s | 6 / failed 3, capped 3 | 43 / restarted 3, healthy 10, capped 30 | 43 / restarted 1, healthy 42 | 100 ms | 155 |
| 6 07:23 | 16.52 s | 6 / failed 3, capped 3 | 43 / restarted 3, healthy 11, capped 29 | 45 / restarted 1, healthy 44 | 50 ms | 156 |

Invariant in every run:
- tokens consumed: hang 3, flap 3, crash 1. hades-gate replay ledger: exactly 7 `consumed`.
- Global `gaia-restarts.jsonl`: 7 lines.
- Unused tokens left: 2 / 2 / 4.
- 0 tick errors, 0 panics.
- Every sleep was within 300-500 ms (observed 300..500).
- Loop children: 0 zombies. Reaped: hang 3, flap 3. The crash loop has 1 live child, the healed instance.
- One-shot tick with a new `--state`: exit 5 capped, token not consumed. Fourth loop on another new `--state`: `{"capped":3}`, 0 restarts.
- Nerve: `gaia.loop.started` 4, `gaia.loop.stopped` 4, `gaia.heal.applied` 4, `gaia.heal.failed` 3, `gaia.heal.approved` 7, `gaia.heal.refused` 0.
- No token text in stdout or the nerve log. All `ts` values end in +02:00.
- Teardown: 0 dummies; 20300 and 19601-19603 closed.

Hang tick durations: 4.1 s while restarting (1 s probe timeout + 2.5 s health wait + spawn), 1.0 s once capped (probe timeout only). Flap and crash ticks: 0-5 ms, or about 205 ms for a tick with a restart.

### Verbatim output, run 6 (final code)
```
=== run 6 2026-10-21 07:23:59 +0200

running 1 test
[2026-10-21T07:24:07+02:00] start openmind-nerve on 127.0.0.1:20300
nerve GET /health: {"appended":0,"cycle":1,"score":0,"service":"openmind-nerve","status":"ok","system":"failing","ts":"2026-10-21T07:24:07+02:00","version":"0.1.0"}
[2026-10-21T07:24:07+02:00] dummies up: hang 19601 flap 19602 crash 19603
[2026-10-21T07:24:07+02:00] 3 loops started, pids [4147531, 4147532, 4147533]
[2026-10-21T07:24:24+02:00] all three outcomes on the nerve after 16.516848317s
loop pid 4147531: children 0, zombies 0
loop pid 4147532: children 0, zombies 0
loop pid 4147533: children 1, zombies 0
[2026-10-21T07:24:25+02:00] all loops stopped 50 ms after the stop file
dummy-hang: ticks 6 restarts 3 outcomes {"apply-failed":3,"capped":3} tokens_spent 3 reaped 3 sleeps 321..498 ms errors 0
dummy-hang: first tick durations ms [4122, 4161, 4148, 1004, 1022, 1024]
dummy-flap: ticks 43 restarts 3 outcomes {"capped":29,"healthy":11,"restarted":3} tokens_spent 3 reaped 3 sleeps 303..499 ms errors 0
dummy-flap: first tick durations ms [0, 0, 1, 1, 204, 1]
dummy-crash: ticks 45 restarts 1 outcomes {"healthy":44,"restarted":1} tokens_spent 1 reaped 0 sleeps 300..492 ms errors 0
dummy-crash: first tick durations ms [0, 1, 5, 0, 0, 0]
[2026-10-21T07:24:25+02:00] one-shot heal-tick with a new --state dir: exit 5 capped, token not consumed
[2026-10-21T07:24:25+02:00] second flap loop on a new --state dir: outcomes {"capped":3} (no restart, no token)
nerve events: 156 total; loop.started 4 loop.stopped 4 heal.applied 4 heal.failed 3 heal.capped 36 heal.approved 7 heal.refused 0
dummies alive before teardown (ours): 2
[2026-10-21T07:24:26+02:00] teardown: 0 dummies, ports 20300 + 19601-19603 closed
test real_supervised_loop_hang_flap_crash_global_budget ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.91s

```

## Regression and gates (final code, 07:23-07:25)
- `real_gaia_live` (the earlier M2 real test, GALION_REAL=1, its own fixed ports 19233/19234, checked free first) passed in 1.45 s with the global ledger in place: restart 1/2/3, replay/forged/no-token refused, 4th capped.
- CLI scope fail-fast with the x86_64 musl binary (07:25:06): `gaia heal-tick --loop ... --service hermes-rs --port 8642 --allow-ports 1-65535` returned `"stop":"scope-refused","exit":4,"ticks":0`. Nothing was probed or posted, and no state dir was created.
- Unit/hermetic tests:
  - gaia + gaia-live: 67 passed / 0 failed.
  - New: heal_loop 11, heal_tick +3 (global cap with a fresh `--state`, corrupt global ledger fails closed, refusal codes), gaia-live heal +6 (path, union and dedup, new state sees global, torn ledger error, lock exclusive across threads, 8 x 25 concurrent records with no torn line).
- Workspace (`--exclude openmind-sandbox --exclude cgroup-detect`, 07:16): 1245 passed, 0 failed, 6 ignored.
- `cargo fmt --check` ok. `cargo clippy --workspace --all-targets -D warnings` ok. `scripts/no-python.sh` ok. `scripts/spdx-scan.sh` missing 0. `repo-check` PASS, 0 errors, 47 warnings (base 625eaf0: 47).
- Builds:
  - x86_64-unknown-linux-musl release (static-pie): gaia 1,033,088 B, gaia-dummy 475,936 B.
  - aarch64-unknown-linux-musl release (static, rust-lld): gaia 875,272 B, gaia-dummy 416,656 B.
  - `cargo check` ok for armv7-unknown-linux-musleabihf and aarch64-linux-android.

## VPS, read-only (nerve observation per no_skip_extensive_testing)
`ssh slavik-vps`, GETs and listings only:
- 07:13:55 `curl 127.0.0.1:8670/healthz` = `ok`.
- 07:14:05 `/state`: mode `live`, cycle 6799, `interval_s` 30, `cycle_ms` 443. The `gaia` component is `absent` (Gaia is not installed on the VPS, as expected). Reflex limits: `restarts_per_hour_default 3`, `cloudflared_per_hour 1`, `max_actions_per_cycle 2`, `fail_cycles 3`, `rollback_per_unit_per_day 1`. `reflex.recent` was empty. `systemctl list-timers` showed no guardian/heal/gaia timer and `/usr/local/bin` has no gaia binary.
- 07:25:05 `/state` after all box runs: cycle 6821, 11/15 healthy. Not healthy: galion-one-mouth degrading, cloudflared degrading, voice-worker absent, gaia absent. These are the same as before the runs. Sum of `restarts_1h` is 0 and `reflex.recent` is empty, so the box work caused no VPS restarts.
- The cortex is not deployed, so the nerve was used, as the standing rule says.
- Comparison: the nerve reflex's 3 restarts per unit per hour matches Gaia's `MAX_RESTARTS_PER_HOUR` = 3. The two budgets are separate (owner decision D-L2 in docs/gaia-heal-loop.md).

## Resume re-verification (08:08-08:12, same code as ea5b53b)
A new executor resumed the slot after the first one was interrupted at about 07:25. The code was not changed; this section only adds evidence.
- `cargo fmt --check` ok. `cargo clippy --workspace --all-targets -- -D warnings` ok.
- Workspace (08:08:40-08:09:18, `--exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`): 262 suites, 1234 passed, 0 failed, 6 ignored.
- gaia + gaia-live: 67 passed / 0 failed (heal_loop 11, heal_tick 13, gaia-live heal 12, others unchanged).
- **Disclosure:** the 07:16 workspace run above (1245 passed) did not exclude hermes-tools. Its sandboxed tests may have run the sandbox locally once, which goes against sandbox_vps_only (same slip as slots 150 and 155). There was no host change. The 08:08 run excludes it.
- `real_gaia_loop` (GALION_REAL=1, release), 3 more passing runs. Run 7 started 08:09:54, run 8 08:10:21, run 9 08:10:40. Settle times were 16.51, 16.52 and 16.03 s, and each test run took 18.5-19.0 s. Every run held the same invariants: hang 6 ticks (apply-failed 3, capped 3); flap 44-45 ticks with restarted 3; crash 42-44 ticks with restarted 1; tokens 3/3/1; one-shot exit 5 capped; fourth loop `{"capped":3}`; nerve loop.started 4, loop.stopped 4, heal.applied 4, heal.failed 3, heal.approved 7, heal.refused 0; 0 errors; sleeps 301-500 ms; the stop file stopped the loops in 51-101 ms. Teardown: 0 dummies; 20300 and 19601-19603 closed.
- `real_gaia_live` regression (08:11:28) passed in 1.48 s. Ports 19233/19234 were free before and closed after.
- VPS, read-only, 08:11:12-08:11:38: nerve `/healthz` returned `ok`; `/state` mode `live`, cycle 6914, statuses healthy 17 / degrading 4 / absent 3 (all components, nested included), `restarts_1h` 0 on all 15 units, `reflex.recent` []. No gaia/heal/guardian timer, no gaia binary in /usr/local/bin. Cortex `/cortex` returned 404 (not deployed), so the nerve was used.
