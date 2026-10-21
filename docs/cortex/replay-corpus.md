<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex replay regression corpus (plan T5.3), 2026-10-21 +02:00

Code: `crates/openmind-nerve/src/corpus.rs` (verdict, flap rule, fingerprint, manifest, diff),
`src/replay.rs` (`replay_dir_with`: hold cycles + per-cycle hook + pinned sensor env),
`src/engine.rs` (`Engine::new_with_env` / `with_specs_env`), `src/main.rs` (`replay DIR --verdict [--hold N]`),
`tests/replay_corpus.rs`, `tests/real_replay_corpus.rs`, `crates/xtask` (`cargo xtask replay-corpus`),
`.github/workflows/self-eval.yml` job `replay-corpus`. Base: `swarm/cortex-t32-correlate` ae604dc. No new dependency.

## What it does

Every saved capture is replayed through the live pipeline and compared with a committed expected verdict.
A change in the cortex (scoring, correlation, sensors, thresholds) that moves any verdict now fails a test with a
path-by-path diff, instead of being noticed by hand in the next live capture.

| piece | where |
|---|---|
| manifest: every capture, its source and its sensor env | `crates/openmind-nerve/tests/replay-corpus/corpus.json` |
| expected verdict per capture | `crates/openmind-nerve/tests/replay-corpus/expected/<name>.json` |
| synthetic captures (committed) | `tests/fixtures/cortex-replay`, `tests/fixtures/cortex-sshd` |
| live VPS captures (box only, never committed) | `/workspace/swarm/nerve-capture/replay{1,158,179,186,192,193}` (`NERVE_CAPTURE_ROOT`) |

Each run: capture cycles (one per `state-*.json`), then **20 hold cycles**: the last snapshot again, `NERVE_INTERVAL`
(30 s) apart, no new access hits, journal lines only where the capture has them (a quiet host that stays as
captured). Per cycle the verdict is reduced to:

- `score`, `grade`, `pass`, `system`;
- incidents: cause, severity, correlated member findings (T3.2);
- cortex findings `category:id severity`;
- self-eval checks `level check component`;
- deductions `points what`.

All text is normalised: every digit run becomes `#` (no addresses, counts or ages in the repo), embedded JSON
health bodies are re-serialised with sorted keys (serde_json `preserve_order` differs between `-p` and
`--workspace` builds). The expected file holds the verdict after the capture (`final`), after the hold (`held`),
the `score grade` timeline of all 23 cycles, the flap list, and a fingerprint of the capture (file, size,
FNV-1a 64), so a changed capture is reported as "the capture itself changed", not as code drift.

## The test fails when

1. a saved capture is not in the manifest (every dir with a `state*.json` under `tests/fixtures/` and under the
   live root, when the live root exists), or an expected file has no entry / an entry has no expected file;
2. any verdict field drifts (`$.result.final.score: 16 -> 15`, `deductions: lost "4 vulnerability (warn) ..."`);
3. a replay is not deterministic (each entry replays twice);
4. **a key flaps**: an incident (with its severity), finding (with its severity), check or grade that clears and
   comes back again over the capture plus 20 hold cycles (more than one episode). One onset and one clear (a flood
   that stops, a scanner window that ages out) is not a flap.

Positive controls in the same test file: a doctored expected file is caught (`drift_is_detected`); a capture where
hermes-rs fails 4 snapshots, recovers 4, fails 4 is reported as a flap and the binary exits 3, while failing once
is not a flap (`a_flapping_capture_is_caught`, and the binary version in `real_replay_corpus.rs`).

The sensor settings of a capture (e.g. `NERVE_SSHD=1` for the sshd fixture) come from the manifest, never from the
process environment: before this branch `Engine::new` read the S2 sensor switches from the process env, so a shell
with `NERVE_SSHD=1` or `NERVE_SURFACE_TARGETS` set replayed captures differently. `Engine::new` is unchanged for the
live service (process env); replay paths now take an explicit env.

## Commands

```
cargo xtask replay-corpus                  # what CI runs (fixtures; live captures skipped when absent)
cargo xtask replay-corpus --require-live   # box: every live capture must be there
cargo xtask replay-corpus --bless          # intended verdict change: rewrites expected/, prints the diff
openmind-nerve replay DIR --verdict        # one capture: final/held/timeline/flaps JSON, exit 3 on a flap
NERVE_SSHD=1 openmind-nerve replay crates/openmind-nerve/tests/fixtures/cortex-sshd --verdict --hold 60
```

Adding a capture: capture read-only (3x `GET :8670/state` 30 s apart, `GET :8660/status.json`, `ss -ltn`,
`journalctl -o json --output-fields=UNIT,_SYSTEMD_UNIT,MESSAGE,PRIORITY,__REALTIME_TIMESTAMP --since -30min`) into
`$NERVE_CAPTURE_ROOT/<name>`, add an entry to `corpus.json`, run `--bless`, review the new expected file, commit.
The corpus test fails until a new capture dir is listed.

## Corpus today (2026-10-21 15:25 +02:00)

| entry | cycles | final | held (after 20 quiet cycles) | incidents (final) |
|---|---|---|---|---|
| fixture-cortex-replay | 3 + 20 | 16/F | 41/F (ddos:tuner clears) | ddos:tuner, edge, galion-desk, galion-one, scan:console |
| fixture-cortex-sshd (`NERVE_SSHD=1`) | 1 + 20 | 67/D | 100/A (flood window ages out) | ssh flood (critical) |
| live-replay1 (2026-10-20 19:25) | 3 + 20 | 56/D | 62/D (scan:console ages out at hold cycle 8) | edge, galion-one, scan:console |
| live-replay158 (2026-10-21 08:24) | 2 + 20 | 66/D | 66/D | edge, galion-one |
| live-replay179 (09:16) | 3 + 20 | 66/D | 66/D | edge, galion-one |
| live-replay186 (12:39) | 3 + 20 | 66/D | 66/D | edge, galion-one |
| live-replay192 (14:48) | 3 + 20 | 66/D | 66/D | edge, galion-one |
| live-replay193 (15:22, this slot) | 3 + 20 | 66/D | 66/D | edge, galion-one |

No entry flaps. Mutation check: changing one score constant (`Vulnerability, Warn` 4 -> 5 in `score.rs`) fails the
corpus on all 8 captures with the exact lost/new deductions and score deltas (reverted).

## Limits and owner decisions

- **D-T53-1 live captures in CI.** The 6 live captures stay on the box (real journal lines: ssh source addresses,
  session names), so CI replays only the 2 synthetic fixtures. Options: (a) keep as is (box run with
  `--require-live` before merges); (b) a Rust redactor (addresses to RFC 5737 ranges, user/session names to
  placeholders) and commit redacted, trimmed captures (about 6 MB raw today); (c) keep them in a private artifact
  store. Recommendation: (b) for one or two captures, after review of a redacted sample. Costs no money.
- **D-T53-2 capture gap.** A `/state` capture has no pushed signals: the live nerve at 15:30 +02:00 (cycle 695)
  scored 61/D with `spend-cap:hermes-rs` open, the replay of the capture taken a minute earlier 66/D without it
  (same `edge` and `galion-one`). Options: add `GET /api/v1/incidents` (or the signal log) to the capture and teach
  replay to inject captured signals; or accept the gap and document it. Recommendation: add it (small, read-only).
- **D-T53-3 flap rule and hold length.** Current rule: more than one episode of a key over capture + 20 hold cycles
  of 30 s (10 min). Alternatives: count grade changes separately, or a longer hold (60 cycles = 30 min, about
  3x the run time, still under 0.1 s per capture in release).
- **D-T53-4 xtask merge order.** This branch adds a minimal `crates/xtask` (one task) with the same package name,
  alias and `main.rs` shape as `swarm/ci-workflow-rust` 38a26cb (full runner, on integration-2). Whoever merges
  second keeps the full runner and adds `mod replay_corpus;` plus the `"replay-corpus"` match arm; the alias line
  in `.cargo/config.toml` is identical.
- Observation (no decision): incidents open on the first failing snapshot and clear one cycle after recovery
  (flap control timeline `60 D x5, 85 B x3, 60 D`). Live status already carries the nerve's own fail-cycle logic.

## T3.1 update (2026-10-21 20:05 +02:00, swarm/cortex-t31-calibrate, slot 207)

The corpus now has 17 entries: `live-replay200` (slot 200's 19:00 capture; it was unlisted and failed
`every_saved_capture_is_in_the_corpus`), `live-replay207` (fresh read-only capture 19:51 +02:00; the live nerve runs
in event mode, so the 3 GETs gave one identical snapshot and one is kept), a `NERVE_SSHD=1` twin of the 6 live
captures with ssh lines (`live-replay158-sshd` .. `live-replay207-sshd`), and the synthetic
`fixture-cortex-t31-band`. Thresholds tightened by T3.1 (sshd `invalid_user` 30 -> 20, memory-heavy 512 -> 256 MiB)
move only the band fixture (100/A -> 96/A); every other entry is unchanged, 0 flaps. Live verdicts: replay200 76/C
final, 66/D held (galion-one passes its 900 s loading grace during the hold); replay207 66/D. See
docs/cortex/calibration.md.
