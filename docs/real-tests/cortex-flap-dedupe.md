<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-flap-dedupe (slot 235, T5.1 finding F1 follow-up)

Branch `swarm/cortex-flap-dedupe`, base `swarm/cortex-t51-chaos` e982210.
Box only, loopback only, ports 21050-21059. Nothing deployed; the VPS was
only read (GET on 127.0.0.1 over ssh, `systemctl show`).

## The problem (F1, reproduced here as "before")

The event-mode nerve (`swarm/nerve-event-driven` bb287cc, live on the VPS)
folds every event whose `key` it saw within `dedupe_s` (default **600 s**,
also live) into the stored event: the ring copy's `count` goes up, nothing
new goes on the stream. A unit in a crash loop is ONE stream event for as
long as the loop lasts, so the cortex flap rule (3 failures in 900 s) could
never fire through the real nerve. Also: the nerve applies its executor
caps (2/unit/h, 6/h) only to EXECUTED restarts, so in dry-run nothing
bounded simulated restarts except the cortex's own 3/unit/h.

## The fix (cortex only; the live nerve bb287cc needs no change)

1. **Occurrences count.** `Event::occurrences()` = the nerve `count` (>= 1).
   `Subject::failures()` sums occurrences instead of counting events, so an
   event replayed after a reconnect with `count 4` is 4 failures.
2. **Repeat watch** (`crates/gaia-cortex/src/repeat.rs`). Every unit failure
   from the nerve that the cortex decided on is watched by `seq`. While at
   least one is watched, a poller reads `GET <nerve>/evt?since=<min-1>&n=1000`
   (the nerve's own ring endpoint, paged, max 4 pages) every
   `repeat_poll_s` (default 30 s, `CORTEX_REPEAT_POLL_S`, 0 = off). A `count`
   rise becomes a repeat event for the decider (`count` = the new
   occurrences, `data.repeat = {of_seq, total, new}`). Idle = condvar wait, no
   calls. A watch ends `flap_window_s` after its last rise, when the event left
   the ring, or when the seq now names another event / the count went down
   (restarted nerve); `nerve_restarted()` clears it. At most 64 watched.
3. **Caps in every mode.** `restarts_per_unit_hour` default 3 -> **2** and new
   `restarts_total_hour` **6** (= the nerve's caps, constants in the contract
   crate); restarts that the nerve only simulated count like executed ones
   (they already did), so a dry-run loop is bounded exactly like a live one.
   New rule `R3-cap-total` (escalate, no circuit, no action).
4. Contract: `.path`, `.automount`, `.swap`, `.device` units are no longer
   renamed `<x>.service` (found on live VPS data, below).

`/stats` gains `events_repeat`, `restarts_last_hour`, `caps`, `repeat_watch`
(`poll_s, watching, watched_total, polls, poll_errors, repeat_events,
repeats, expired, left_ring, replaced, last_poll`). Decisions gain
`failures` and `repeat_of`.

## Real test: `crates/gaia-cortex/tests/real_flap_dedupe.rs`

```
GALION_REAL=1 GALION_NERVE_BIN=<release openmind-nerve built from bb287cc> \
  cargo test --release -p gaia-cortex --test real_flap_dedupe -- --nocapture --test-threads 1
```

Real release `openmind-nerve serve --mode event` (bb287cc, built here with
`--locked`, 1,312,872 bytes), `NERVE_EVT_DEDUPE=600` as on the VPS,
`NERVE_DRY_RUN=1`, journal from a FIFO (the nerve's own parser), plus the
real release `gaia-cortex`; a fresh pair per phase on 21050 / 21051.

Runs: 2026-10-22 00:17, 00:18, 00:19 (first test version) and the final
version 00:21:49, 00:22:42, 00:23:36 (+02:00): **6/6 green**, 53.3-53.8 s each.

| phase | setup | measured (final 3 runs) |
|---|---|---|
| before | `CORTEX_REPEAT_POLL_S=0` (the e982210 transport); `tuner.service` SIGKILL exit + failed, then 5 more failures 700 ms apart | nerve `deduped +5`, ring `unit.failed` count **6**; cortex decisions `[R3-known-crash]` only, circuit closed, 1 dry-run restart request: **F1 reproduced** |
| after | `CORTEX_REPEAT_POLL_S=2`, same journal | `[R3-known-crash, R2-circuit-open]`; R2 decided after the 3rd / 4th / 3rd folded failure, **1988 / 2496 / 2097 ms** after the first folded write; reason `tuner.service flapping: 4 failures in 900s >= 3; circuit open 3600s, human needed`; escalated; `repeat_of [seq]`; still 1 restart request (none while flapping); repeat_watch `polls 4, repeat_events 2, repeats 5, poll_errors 0` |
| caps, cortex dry-run | `flap_failures 100` (flap rule out of the way), one folded failure at a time | tuner: `R3-known-crash, R3-known-crash, R3-cap`; nerve saw exactly **2** restart requests, both answered `dry-run` (the nerve itself never rate-limits in dry-run) |
| total cap, cortex dry-run | then 6 more allowlisted units killed one by one | `R3-known-crash` x4, `R3-cap-total` x2; nerve restart requests **6**; `restarts_last_hour 6` |
| caps, cortex live | `CORTEX_ACTIONS=live` against the dry-run nerve | tuner: `R3-known-crash, R3-known-crash, R3-cap`; 2 requests, both `dry-run`; `actions_executed 0` |

Teardown: processes killed by the test, work dirs removed, ports 21050/21051
re-bound by the test at the end (free).

## Live data check (VPS, read-only, 2026-10-22 00:18:13 +02:00)

- Nerve `:8670` `/stats`: event mode, started 2026-10-21 22:56:55, seq 39,
  `deduped 11`, `dedupe_s 600`, `actions_live true`, streams 1.
- Nerve `/evt?since=0&n=1000`: 39 events, 7 with `count > 1`, among them a
  real folded failure: seq 12 `unit.failed gaia-hermes-handoff.path`
  (`unit_result trigger-limit-hit`) **count 2** at 23:06:25, plus seq 18
  `unit.start_failed` for the same unit at 23:08:13: three failures in
  ~2 min that the stream delivered as two events.
- Live cortex `:8674` (started 23:36:53, `mode live`, llm on, hermes handoff
  on): events_in 6, decisions 5, actions 0; it started after that failure,
  so it never decided on it.
- Those 39 live events replayed through both builds (`gaia-cortex replay`,
  box, dry-run, data kept off the repo):
  - e982210: `unit:gaia-hermes-handoff.path.service` (wrong unit name) **R4-unknown-unit**, 2 events;
  - this branch: `failures 3` -> **R2-circuit-open** (flapping, escalate) and,
    after the suffix fix, the unit keeps its real name `gaia-hermes-handoff.path`.
  Every other subject decided the same in both builds.

## Gates (2026-10-22 00:24 - 00:29 +02:00)

- `cargo fmt --all --check` ok; clippy `-D warnings --all-targets` for gaia-cortex +
  nerve-cortex-contract, default and `--features gaia-cortex/llm-advisor`: 0 warnings.
- `cargo test --release -p gaia-cortex -p nerve-cortex-contract`: lib 14 (+5 repeat
  watch), binary_e2e 1, **flap_dedupe 7 (new)**, pipeline 22, contract 9 (+1), all pass;
  real tests gated (skip without `GALION_REAL=1`).
- Workspace (excl. openmind-sandbox, cgroup-detect, hermes-tools; `--no-fail-fast`): 230
  suites, **1141 passed / 1 failed / 4 ignored**. The 1 failure is the same box-state
  failure as at e982210: `openmind-nerve replay_corpus every_saved_capture_is_in_the_corpus`
  (`live:replay200`, `live:replay207` saved by other slots in `/workspace/swarm/nerve-capture/`).
- `real_t51_chaos` was NOT re-run: its ports 20970-20979 belong to slot 227. It now starts
  its cortex with `CORTEX_REPEAT_POLL_S=0` so S2a keeps recording F1; S8's `<= 3`
  restarts check still holds with the cap of 2.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 420 scanned / 0 missing; repo-check
  4 errors / 44 warnings, identical to e982210 (pre-existing test IPs), none in changed files.
- x86_64-unknown-linux-musl release 938,848 bytes (sha256 `768ce54d69ec0666…`, static-pie);
  aarch64-unknown-linux-musl 804,776 bytes (`0e0a5cfa50a7236b…`, rust-lld);
  `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## Owner decisions

- **D-235-1** Repeat-watch poll interval: default 30 s (a crash loop is seen
  within ~30 s plus coalesce). Shorter = faster flap detection, more loopback
  GETs only while a loop is open. Alternative later: a nerve-side change that
  re-emits a `unit.repeat` event every N folds (needs a nerve deploy).
- **D-235-2** Cortex per-unit cap 3 -> 2/h and new total cap 6/h, equal to the
  nerve executor caps so dry-run and live behave the same. Keep, or set other
  numbers in the cortex config.
- **D-235-3** Nerve-side: apply the executor caps before the dry-run answer
  (report `would be rate-limited`)? Not done here (nerve untouched, live
  bb287cc); the cortex caps now cover it.
- **D-235-4** Deploying this cortex build to the VPS (replacing the live
  cortex) is NOT part of this slot.
