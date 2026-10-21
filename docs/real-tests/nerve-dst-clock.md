<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T3.4: DST fall-back fake-clock replay (slot 157, swarm/nerve-dst-clock)

Base: `swarm/cortex-t20-merge` ee75f75. All times Europe/Zagreb.
The fold: 2026-10-25 03:00:00 CEST (+02:00) becomes 02:00:00 CET (+01:00),
unix 1 792 890 000. The hour 02:00-02:59 happens twice on the wall clock.

## What was audited

| area | how it keeps time | across the fold |
|---|---|---|
| self-eval / state `ts` | `zagreb_time::from_unix` (chrono-tz) | correct: `02:59:30+02:00` then `02:00:00+01:00`, strictly increasing when parsed |
| **self-eval file names** | `ts` with `:`/`-` removed, **cut to 15 chars (offset dropped)** | **BUG**: 02:x9:30 CET overwrote 02:x9:30 CEST: **6 reports lost** per fold at the 10 min cadence; retention sorted by name, not by instant |
| 2 h sample history (240 x 30 s) | unix seconds | correct: span 7170 s across the fold |
| access-rate buckets (720 x 10 s = 2 h) | unix seconds from CLF `+0200`/`+0100` and RFC 3339 stamps | correct: every hit lands on its own instant |
| restart detection | `NRestarts`, MainPID, `ActiveEnterTimestampMonotonic`, `restarts_1h` on unix | correct: a restart at 02:50+02:00 counts until 02:50+01:00 (one real hour), then 0 |
| memory trend (30 min, same pid) | unix seconds | correct: detection delay 1440 s (control night) / 1410 s (CEST pass) / 1410 s (CET pass); "over N min" is real minutes |
| error rate (5 min), DDoS baseline (30 min) | unix seconds | correct: no false alarm; a flood at 02:10+01:00 is flagged 30 s later and clears |
| journal window | `journalctl --since @<unix>` | correct (no local-time argument) |
| replay of captures | sorted by parsed `ts`, not by file name | correct: names 0210 < 0220 < 0240 < 0250 replay as 02:40+02, 02:50+02, 02:10+01, 02:20+01 |

## Fix (crates/openmind-nerve/src/engine.rs)

- `self_eval_file_name(ts)`: keeps the offset, e.g.
  `self-eval-20261025T020930+0200.json` and `self-eval-20261025T020930+0100.json`.
- `self_eval_file_instant(name)`: parses the offset form; legacy 15-char names give `None`.
- `self_eval_prune(files, 96)`: keeps the newest 96 by instant (legacy names by mtime), ties by name.
- `access::Stream::window()`: read-only view of the kept buckets (count, first, last start) for the tests.

No new dependencies (std + existing serde_json/zagreb-time). No behaviour change
outside the self-eval file name and its retention order; the cortex-replay
goldens are unchanged.

## Tests

- Unit (engine.rs): `self_eval_names_are_unique_across_the_october_fold`
  (25 reports over 4 h: all unique, each name parses back to its instant; the
  old 15-char name collides on exactly 6), `self_eval_prune_keeps_the_newest_by_instant`.
- Fake-clock replay `tests/dst_clock_replay.rs` (5 tests, `Engine::cycle_with`
  every 30 s from 2026-10-24T23:00:00+02:00 to 2026-10-25T04:00:00+01:00,
  721 cycles, full VPS component set, CLF + JSON access logs, journal lines):
  - quiet night: 0 incidents, score 100 constant, findings unchanged, ddos all
    normal, `restarts_1h` 0, no restart/memory/error reasons, 0 alerts, pass;
    37 self-eval files on disk = 37 report cycles, 12 of them in the 02:xx hour.
  - leak (control 01:15+02:00, 02:45+02:00, 02:45+01:00): delays [1440, 1410, 1410] s.
  - restart at 02:50+02:00: `restarts-tuner` exactly from 02:50:00+02:00 to 02:50:00+01:00.
  - flood at 02:10+01:00: flagged 02:10:30+01:00, normal within 240 s.
  - replay_dir ordering across the fold.
- Negative control: with the old name (offset cut) the quiet-night test fails
  `a report was overwritten`: **31 files for 37 reports**.

Runs (2026-10-21, box): `cargo test -p openmind-nerve --test dst_clock_replay` 5/5;
`openmind-nerve` lib 115/115; workspace (excl. openmind-sandbox, cgroup-detect)
997 passed, 0 failed, 3 ignored; clippy `-D warnings` workspace; fmt; no-python ok;
spdx-scan 0 missing; musl x86_64 + aarch64 release build; armv7-musleabihf +
aarch64-android check.

## Real-life test (`tests/real_dst_clock.rs`, `GALION_REAL=1`, `--release`, ports 20270-20271)

```
GALION_REAL=1 NERVE_DST_SSH=root@<vps> NERVE_DST_SSH_KEY=<key path> \
  cargo test --release -p openmind-nerve --test real_dst_clock -- --nocapture --test-threads 1
```

1. `serve` on 127.0.0.1:20270, live HTTP thread on :20271, 5 s cycles,
   report every cycle, 100 legacy self-eval files pre-seeded (mtime a day old).
2. Three read-only `GET 127.0.0.1:8670/state` snapshots of the live VPS
   galion-nerve (30 s apart, over ssh), re-stamped every 30 s through the fold
   (02:25+02:00 .. 02:35+01:00, 141 cycles, tunnel counters kept monotonic)
   and over an ordinary night (2026-10-21 02:25 .. 03:35+02:00), each replayed
   by the release binary (`openmind-nerve replay`). The snapshots are not stored.

| run (CEST +02:00) | serve | live snapshots | fold replay | control replay |
|---|---|---|---|---|
| 07:09:24-07:11:27 | 96 files kept, 3 new `self-eval-20261021T0710{06,11,16}+0200.json`, `/self-eval` 07:10:16+02:00 score 100 pass | 07:10:05/35, 07:11:05 +02:00, 15 components, system degrading | ts 02:35:00+01:00, score 85, causes {galion-one} | score 85, causes {galion-one} |
| 07:18:59-07:21:06 | 96 kept, 3 new (07:19:46/51/56+02:00) | 07:19:35, 07:20:05, 07:21:05 | score 60, causes {ddos:edge-cloudflared, galion-one} | identical |
| 07:21:35-07:23:03 | 96 kept, 3 new (07:21:42/47/52+02:00) | 07:21:35, 07:22:05, 07:22:35 | score 80, findings {memory-heavy-console, stuck-loading-galion-one-mouth}, causes {edge, galion-one} | identical |

Run 2's `ddos:edge-cloudflared` came from the harness, not from the VPS. Cycling
3 snapshots made the cumulative tunnel counter go backwards every third cycle,
and the third snapshot came 60 s after the second. It showed up in both runs
alike, so it is not a DST effect. Run 3 rebases the counters to a monotonic
per-cycle rate and asserts there is no `ddos:` cause. Its `edge` incident is
real: the live nerve reported `cloudflared: degrading: tunnel origin errors
1142/1447 requests in 10 min` and `galion-one-mouth: health probe HTTP 503`
at 07:23 (read-only GET).

## Live VPS (read-only, 2026-10-21 07:01-07:23 +02:00)

- `galion-nerve.service` (8670) active, MainPID 1466797, NRestarts 0, started
  2026-10-18T22:35:04+02:00; `galion-nerve-shadow.service` (8672) active,
  MainPID 4008991, NRestarts 0. Same before and after the tests.
- Live self-eval names use the **old** offset-less format:
  `/var/lib/galion-nerve/self-eval/self-eval-20261021T065435.json` (98 entries = 96 + latest.{json,md}),
  shadow `self-eval-20261021T065648.json` (17). Both live nerves will lose six reports
  each on 2026-10-25 between 02:00 and 03:00 unless this build is deployed first.
- Nerve `/self-eval`: 8670 ts 07:14:35+02:00 pass true system degrading (older build, no score field);
  shadow 8672 ts 07:20:48+02:00 score 65 D, system degraded. The cortex runs inside the nerve; no
  separate cortex service is live, so the nerve was the observation point.
- One probe wrote a temporary file `/tmp/.n157` on the VPS (a copy of `/state` to grep) and removed it
  in the same command; later probes piped instead. No service, unit, permission or /opt/galion/openmind touched.

## Open points (not fixed here)

1. JSON access-log stamps **without** an offset (`2026-10-25 02:30:00`) are read as UTC by
   `util::parse_rfc3339` (offset `""` = 0). Every log the nerve reads today has an offset (CLF `+0200`,
   tuner JSON unix seconds), so nothing is wrong now. An offset-less local log would be 1-2 h off and
   would jump at the fold.
2. `replay_dir` feeds all access-log hits on the first cycle. The cortex-replay golden depends on that,
   so it was left alone. Long multi-hour captures with access logs therefore see future hits early.
3. Reflex budgets and alert dedup use the real clock (`now_secs()`), not the cycle's `now`. This is
   harmless live and in replays (reflexes and alerts off), but a fake-clock run with reflexes on would
   mix two clocks.
