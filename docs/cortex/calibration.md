<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex threshold calibration (plan T3.1), 2026-10-21 +02:00

Branch `swarm/cortex-t31-calibrate`, base `swarm/cortex-t53-replay-corpus` 55f07ff. Slot 207. Box only; the VPS
was read only (GET, `ss -ltn`, `journalctl`). No new dependency.

Rule (B41, docs/cortex.md section 5.1): thresholds may be **tightened** after real 300-second windows are stored;
they must **never be loosened in code**. Every threshold below fires at `value >= line` (or `>`), so tightening
means a lower number. `calibrate::CALIBRATED` lists each line with its design value; a unit test fails if a
current value is above its design value.

## How it was measured

A replay judges a capture only at its 2-3 snapshot times, but each capture holds about 30 min of journal.
`openmind-nerve calibrate DIR...` (`crates/openmind-nerve/src/calibrate.rs`) slides over the whole journal in
10 s steps and, at every step, runs the **real** code paths:

| sensor | measured | how |
|---|---|---|
| `sshd` (B41 4.1/4.2) | every `Counts` key in the last 300 s | the real `SshAuth` (`observe`, no findings emitted) |
| `edge-rate` (T2.12) | top per-hostname failed-request rate (60 s), hot episodes for floors 10/8/6/5/4/3 req/s | the real `EdgeHosts` feeder + access.rs `Detector` |
| `edge` (dead origins) | failed requests/min on refused origin dials, last 300 s | `logscan::insights` |
| cloudflared error rate | journal error lines, last 300 s | `probe::journal_counts` |
| `components` memory-heavy | `mem_kb` per service | the state snapshots |

Output holds counts only (no addresses, account names or hostnames). The record of every live capture is
committed as `crates/openmind-nerve/tests/replay-corpus/calibration.json` and checked by `tests/calibration.rs`.

Data: 8 live captures (2026-10-20 19:25 and 2026-10-21 08:24 .. 19:51 +02:00), 1 539 steps, 933 whole sshd
windows on the 6 captures that carry `ssh.service` lines, 21 state snapshots. `replay207` is a fresh read-only
capture taken by this slot at 19:51 +02:00 (live nerve already `openmind-nerve-bb287cc serve --mode event`,
so its 3 GETs returned one identical snapshot; one is kept).

## Measured (highest 300 s window per capture)

sshd (`NERVE_SSHD=1` semantics; the live nerve runs the sensor off):

| capture | steps | ssh lines | invalid_user | preauth_other max / p95 | key root | key other | max_auth | failed_password |
|---|---|---|---|---|---|---|---|---|
| replay1 | 210 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0 |
| replay158 | 182 | 537 | 2 | 2 / 2 | 64 | 0 | 0 | 0 |
| replay179 | 181 | 307 | **8** | 94 / 92 | 22 | 0 | 0 | 0 |
| replay186 | 212 | 152 | 2 | 5 / 5 | 6 | 0 | 0 | 0 |
| replay192 | 181 | 163 | 3 | 7 / 6 | 6 | 0 | 0 | 0 |
| replay193 | 182 | 181 | 1 | 2 / 2 | 9 | 0 | 0 | 0 |
| replay200 | 210 | 0 | 0 | 0 / 0 | 0 | 0 | 0 | 0 |
| replay207 | 181 | 109 | 1 | 1 / 1 | 8 | 0 | 0 | 0 |

Edge and components:

| capture | host streams | top host rate req/s | top host req/60 s | hot episodes at floor 10 / 8 / 6 / 5 / 4 / 3 | dead-origin failed/min | cloudflared errors / 300 s | console KiB | largest other KiB |
|---|---|---|---|---|---|---|---|---|
| replay1 | 14 | 0.43 | 26 | 0 / 0 / 0 / 0 / 0 / 0 | 8.2 .. 17.4 | 82 .. 174 | 614 508 | 28 416 |
| replay158 | 16 | 5.97 | 358 | **1 / 2** / 2 / 2 / 2 / 2 | 8.0 .. 80.0 | 80 .. 800 | 1 278 052 | 28 336 |
| replay179 | 8 | 3.75 | 225 | 0 / 0 / 1 / 1 / 1 / 1 | 8.0 .. 59.6 | 80 .. 596 | 1 242 880 | 23 864 |
| replay186 | 13 | 0.40 | 24 | 0 / 0 / 0 / 0 / 0 / 0 | 7.6 .. 16.8 | 76 .. 168 | 1 258 364 | 25 452 |
| replay192 | 12 | 2.30 | 138 | 0 / 0 / 0 / 0 / 0 / 0 | 7.8 .. 51.0 | 78 .. 510 | 1 260 348 | 29 832 |
| replay193 | 16 | 0.33 | 20 | 0 / 0 / 0 / 0 / 0 / 0 | 7.6 .. 16.4 | 76 .. 164 | 1 261 344 | 25 392 |
| replay200 | 11 | 2.27 | 136 | 0 / 0 / 0 / 0 / 0 / 0 | 7.6 .. 37.2 | 76 .. 372 | 1 268 680 | 24 392 |
| replay207 | 10 | 5.08 | 305 | **1 / 2** / 2 / 2 / 2 / 2 | 7.6 .. 123.0 | 76 .. 1 230 | 1 268 944 | 25 052 |

## Decisions in code (tighten only)

| threshold | design | now | basis | corpus effect |
|---|---|---|---|---|
| `sshd-invalid-user` warn (`sshd::INVALID_USER_WARN`) | 30 / 300 s | **20** | highest real window 8 (replay179): 2.5x headroom; a B41 attack hour (about 83 per 300 s) still fires | none on the 15 real/golden entries; band fixture fires |
| `memory-heavy` info (`components::MEMORY_HEAVY_KIB`) | > 512 MiB | **> 256 MiB** | console 600 MiB-1.27 GiB, every other service at most 29.8 MiB (cloudflared): 8.6x headroom, still only the console | none on real entries (console already flagged); band fixture fires |
| edge-rate per-host floor (`NERVE_DDOS_MIN_RPS`) | 10 req/s | **10 (kept)** | at 8 req/s or lower, replay158 and replay207 each go hot twice in 30 min (2 episodes = a flap under the corpus rule); at 10 one episode | - |

Not changed, with the reason:

- B41 4.2 credential-flood lines (`failed_password`, `distinct_fail_sources`, `top_fail_share_pct`): 0 in every
  real window. Password auth is off on the VPS, so sshd never logs `Failed password`; there is nothing to
  calibrate on. (The pressure shows up as `preauth_other`, which has no threshold: D-T31-2.)
- B41 4.1 `accepted_password >= 1`, `accepted_publickey_other >= 1 / 5`, `max_auth_attempts >= 1`: already at the
  floor (1) or 0 in every window. No basis to move the `>= 5` error line.
- B41 4.3 `bytes_added` (auth.log growth) and the memory-gateway socket rate (`connrate`, B41 3.2): captures
  hold neither (`stat` and `/proc/net/tcp` are live-only reads). Not measurable from the corpus.
- `edge-dead-origins` error at >= 1 failed request/min: the real rate never dropped below 7.6/min in 1 539 steps;
  a lower line changes nothing.
- Nerve status thresholds (`errors_warn_5m` 3 / `errors_crit_5m` 30, disk/RAM, loading grace 900 s): not cortex
  sensor thresholds; cloudflared's error lines (76-1 230 per 300 s, all dead-origin failures) are far above both.

## Proof against the full corpus (20-cycle no-flap test)

The corpus grew from 8 to 17 entries: `live-replay200` (slot 200's capture, which had made the corpus test fail
as unlisted), `live-replay207` (fresh), a `NERVE_SSHD=1` twin of each of the 6 ssh-carrying live captures, and the
synthetic `fixture-cortex-t31-band` (25 invalid users in 300 s and a 300 MiB service: between the old and the new
lines). The new entries were blessed at the **old** thresholds first (commits b5ac853, 2aad011), then the
calibrated code was run against them. The full diff (`cargo test --test replay_corpus`), commit fa5efd8:

```
ok fixture-cortex-replay / fixture-cortex-sshd / live-replay1 / 158 / 179 / 186 / 192 / 193 / 200 and the 5 -sshd twins: unchanged
fixture-cortex-t31-band: verdict drift (9 change(s)):
  $.result.final.findings: new "vulnerabilities:sshd-invalid-user warn"
  $.result.final.findings: new "bleeding:memory-heavy-galion-desk info"
  $.result.final.deductions: new "4 vulnerability (warn): # ssh invalid-user attempts in # s (account enumeration)"
  $.result.final.score: 100 -> 96
  $.result.final.checks / held.checks / held.findings: the same two keys
  $.result.timeline: new "96 A"
```

Reason written in the expected file's commit: the band fixture is the only entry between the old and the new lines.
After the bless: 17/17 entries, 0 flaps (band timeline `96 A` x5, then `100 A` from hold cycle 2 when the
invalid-user lines leave the window: one onset, one clear). Every live capture (8, with and without sshd) and the
golden fixture still open the `edge` incident at **error** (`tests/calibration.rs`,
`live_captures_and_golden_still_flag_the_edge_incident`).

## Owner decisions (Slavik; nothing decided here)

- **D-T31-1 dead-origin bursts trip edge-rate at the current floor.** At 10 req/s, replay158 (5.97 req/s, 358 failed
  requests in a minute) and replay207 (5.08 req/s, 305) go `elevated` once: a warn `ddos:edge-cloudflared` incident
  next to the `edge` dead-origin incident, for the same crawler hitting closed origin ports. The live system shows the
  same incident kind: the live cortex `gaia-cortex` :8674 had `nerve-incident:ddos:edge-cloudflared` warn open
  (opened 19:46:59 +02:00 by the event-driven nerve; not attributed to one burst: the capture's largest bursts were
  at 19:49 and 19:51, 269 and 313 failed requests/min). Loosening is not allowed in code. Options: (a) accept (a burst is a burst); (b) correlation change:
  fold `edge-host-rate` into the `edge` incident when every hot hostname is a dead-origin hostname (no threshold
  moves); (c) remove the dead routes (cloudflared config + DNS, owner), which removes both. Recommendation: (c),
  with (b) until then.
- **D-T31-2 `preauth_other` threshold.** With password auth off, ssh pressure is `[preauth]` disconnects: real windows
  p50 0-3, but replay179 sat at 92-94 per 300 s for its p95. Options: (a) no threshold (today); (b) info at >= 50 per
  300 s; (c) warn at >= 100. Recommendation: (b) after one more capture in a busy hour.
- **D-T31-3 calibration cadence.** Re-run `openmind-nerve calibrate` on every new capture (the corpus test already
  forces each new capture into the manifest); `tests/calibration.rs` fails when a real window eats the 2x headroom
  or when the floor-8 flap evidence disappears, so the next tightening is data-driven too.
- **D-T31-4 sshd sensor live.** The live nerve runs `NERVE_SSHD` off, so the tightened line only matters once it is
  switched on (a live drop-in change: owner).

## Commands

```
openmind-nerve calibrate /workspace/swarm/nerve-capture/replay*      # JSON, counts only
cargo test -p openmind-nerve --test calibration                     # guard (live parts skip without captures)
CALIBRATION_BLESS=1 cargo test -p openmind-nerve --test calibration measured   # rewrite the record
GALION_REAL=1 [GALION_REAL_VPS=1] cargo test --release -p openmind-nerve --test real_calibrate -- --nocapture
```
