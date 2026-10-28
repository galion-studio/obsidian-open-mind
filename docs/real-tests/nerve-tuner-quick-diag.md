<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 420: nerve-tuner-quick-diag (live nerve 68 D, tuner-quick "failing")

Branch `swarm/nerve-tuner-quick-diag` off nerve-merge-6 `9baa450c` (= live
nerve `openmind-nerve-9baa450`). READ-ONLY VPS (GET `/state`, `systemctl
show/status`, `journalctl` reads). Nothing restarted, no config changed,
NOT deployed. All times Europe/Zagreb.

## Verdict

**tuner-quick is really gone, not a timer/oneshot, and the nerve reports it
too harshly.** It is not hurting any user path: `tuner` (8650) is healthy.

| question | answer | evidence (2026-10-28 20:10–20:24 +01:00) |
|---|---|---|
| really down? | yes, permanently: the unit no longer exists | `systemctl show tuner-quick.service`: LoadState=not-found, ActiveState=inactive; `systemctl status`: "Unit tuner-quick.service could not be found."; no unit file in `/etc/systemd/system`, nothing in `/run/systemd/transient` |
| why? | it was a **transient** unit (hand-made `cloudflared tunnel --url http://127.0.0.1:8650`, random trycloudflare URL, see docs/cutover/tuner.md); transient units do not survive a reboot | journal: "Started tuner-quick.service - Temporary trycloudflare tunnel for tuner." 2026-10-18 19:19:30 CEST; last line 2026-10-22 01:17:12 CEST; that boot ended 2026-10-22 10:47:28 CEST; current boot since 2026-10-22 23:46:42 CEST. No stop line: it died with the host |
| timer/oneshot misread? | no | no tuner timers (`systemctl list-timers --all`), only `tuner.service` unit file |
| check bug in 9baa450? | yes, two small ones | (1) `state.rs` returned `failing` for any missing *expected* unit and ignored `critical`, against the `Spec` contract "failing probe on a non-critical component only degrades it" (tuner-quick is `critical:false`); (2) replay fed a not-found component back as present-but-inactive ("degrading: unit inactive"), so replays could not reproduce the live verdict |
| tuner itself | healthy | `/state` tuner: active/running, http 200 on 8650, NRestarts 0 |

## Score 68 D explained (live, cycle 12, ts 2026-10-28 20:10:13 +01:00)

`100 - 17 - 15 = 68` (grade D, D >= 55, C >= 70):

* **17** error incident `edge` = 15 base + 2 for one extra component. The
  components are `cloudflared` (named tunnel routes 17 public hostnames to 9
  closed origin ports, ~61 failed requests/min) **and `tuner-quick`**. So
  tuner-quick costs exactly **2 points**.
* **15** error incident `galion-one` (galion-one-mouth stuck loading, 503).

Without tuner-quick in the edge incident the live score is **70 C** (still
`pass:false`, two error incidents remain). The bulk of the drop is the
closed-origin edge (owner: cloudflared ingress/DNS) and the mouth, not
tuner-quick.

## Staged fix (commit on this branch, not deployed)

1. `crates/openmind-nerve/src/state.rs`: a missing expected unit is
   `failing` only when `critical`, else `degrading` (reason
   `degrading: unit/endpoint not found`). Effect on live: tuner-quick reads
   `degrading`; its signal becomes `warn`; edge stays an error incident
   because of cloudflared, so the score does **not** change from this alone.
   No reflex change: tuner-quick has `restarts_per_hour: 0` and is refused by
   `reflex::permitted`.
2. `crates/openmind-nerve/src/replay.rs`: a captured component whose reason
   ends in `: unit/endpoint not found` replays as a missing sample.
3. `deploy/staged/nerve/components-live-d420.json`: live table
   (`/etc/galion-nerve/components-live.json`, sha256 283db8c3… = staged
   `components-live.json`) with exactly one change: tuner-quick
   `expected:false` + RETIRED note -> status `absent`, out of every
   incident -> live score +2 (68 D -> 70 C). Owner decision D-420-1.

## Tests (box, 2026-10-28 20:20–20:35 +01:00)

* Unit: `state::tests::missing_unit_honours_critical` (tuner-quick missing ->
  degrading; cloudflared missing -> failing; expected:false -> absent);
  `score::tests::incidents_scored_once_and_fail` extended with the live
  arithmetic (edge{cloudflared,tuner-quick}+galion-one = 68 D; edge
  {cloudflared}+galion-one = 70 C).
* `tests/tuner_quick_replay.rs` (real binary `openmind-nerve replay … --state`):
  * `d420_table_changes_only_tuner_quick`: every other spec identical.
  * `replay_of_live_state_fixture`: scrubbed live `/state`
    (`tests/fixtures/tuner-quick/state-20261028-2010.json`, `last_error`
    nulled, components only). Live table: tuner-quick `degrading` /
    `degrading: unit/endpoint not found`, in incident `edge` (warn), score
    80 B; d420 table: tuner-quick `absent`, no incident, score 85 B. (Replay
    has no journal, so the cloudflared closed-port signal is not rebuilt;
    the live +2 is checked in the real leg instead.)
  * `real_live_state_tuner_quick` (`GALION_REAL=1 NERVE_TQ_SSH=slavik-vps`,
    `cargo test --release`, 20:24:13–20:24:33 +01:00), fresh read-only GET:
    ```
    live "2026-10-28T20:10:13+01:00" cycle 12: tuner-quick "failing" ["failing: unit/endpoint not found"] unit_state "inactive" | score 68 "D"
    live edge deduction 17 with 2 component(s); without tuner-quick 15: score 68 -> 70
    replay /tmp/nerve-tq-live-…: live table 80 "B" -> d420 table 85 "B"
    test result: ok. 3 passed; 0 failed
    ```
* Base comparison: the 9baa450 build replaying the same capture gave
  tuner-quick `degrading: unit inactive` (fidelity bug 2), while live says
  `failing: unit/endpoint not found` (bug 1).
* Static x86_64 musl binary, same fixture: live table 80 B / tuner-quick
  degrading not-found; d420 table 85 B / absent.
* Gates: `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -D
  warnings` rc 0; `cargo test --workspace --exclude openmind-sandbox
  --exclude cgroup-detect --exclude hermes-tools` 281 suites, 1349 pass / 0
  fail / 6 ignored; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 560
  scanned, 0 missing; release x86_64-musl (static PIE) and aarch64-musl
  (rust-lld, static EXEC) build; armv7-musleabihf and aarch64-android
  `cargo check` ok.

## Side observation (not changed)

Live `/state` `ts` stayed 2026-10-28 20:10:13 +01:00 between 20:10 and 20:24
(event mode, cycle 12 since start 17:40:13 +01:00): the state is refreshed on
events, not every 30 s. Worth knowing when reading `ts` as liveness.

## Owner decisions

* **D-420-1** (recommend YES): install `components-live-d420.json` as
  `/etc/galion-nerve/components-live.json` at the next nerve deploy (no
  restart just for this). Live score 68 D -> 70 C.
* **D-420-2** (recommend YES): ship the `state.rs` + `replay.rs` fix with the
  next nerve merge (nerve-merge-8).
* **D-420-3** (SEC-5 / O4, owner): republish the tuner via the staged
  `deploy/staged/tuner-publish.service` (random trycloudflare URL) or leave
  it unpublished (recommended: leave unpublished; nobody depends on it since
  2026-10-22, and a random public URL was a security note). If republished,
  flip `expected` back to `true`.
