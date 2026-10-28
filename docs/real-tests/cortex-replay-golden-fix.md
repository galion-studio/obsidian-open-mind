<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# cortex-replay-golden-fix (wave-4 slot 324)

Branch `swarm/cortex-replay-golden-fix` off `cortex-live-merge-3` 73583abf.
Done 2026-10-27 22:53 - 23:05 CET (+01:00). Ports 21940-21949 reserved, none
needed (replay opens no socket). NO deploy, VPS read-only.

## The failure (seen by slots 309 / 310)

On the base, `cargo test -p openmind-nerve --test replay_corpus`:

```
test every_saved_capture_is_in_the_corpus ... FAILED
saved capture(s) not in tests/replay-corpus/corpus.json: ["live:replay200", "live:replay207"]
test corpus_replays_without_drift_or_flaps ... ok
test drift_is_detected ... ok
test a_flapping_capture_is_caught ... ok
```

`cortex_replay` (the cortex golden: 5 tests incl.
`nerve_events_match_the_golden_and_carry_no_titles`,
`replay_matches_the_pre_trait_build`) passes on the base: 5/5.

## Root cause: stale corpus manifest, not a regression

- The test lists every capture dir under the live capture root
  (`/workspace/swarm/nerve-capture`, box only, never committed) and requires a
  corpus entry for each. `replay200` (slot 200, VPS 2026-10-21 19:44 +02:00)
  and `replay207` (slot 207, VPS 2026-10-21 19:52 +02:00) were captured after
  the corpus was made (slot 193) and never listed on this cortex lineage. Only
  `swarm/cortex-staging-fold` (2b5712b4) added them, on a branch this base
  does not contain.
- No engine code changed: all 8 existing expected files replay with zero
  drift and zero flaps on the base, before and after this change
  (`git diff` touches only `corpus.json` + 2 new expected files).
- On CI (no live root) the test passed all along; it only fails on the box.

## Fix

- `corpus.json`: two new entries `live-replay200`, `live-replay207`.
- `expected/live-replay200.json`, `expected/live-replay207.json`: blessed with
  `REPLAY_CORPUS_BLESS=1` from this base. Only these two files were written
  (the bless pass reported "1 change" = "no expected file" for each; the 8
  old ones reported nothing).
  - replay200: final 76 C, held 66 D, 3 capture + 20 hold cycles, 2 incidents
    (edge error: edge-dead-origins + error-rate-cloudflared; galion-one warn),
    4 findings, flaps [].
  - replay207: final 66 D, held 66 D, 1 capture + 20 hold, 2 incidents (edge
    error; galion-one error stuck-loading-galion-one-mouth), 5 findings,
    flaps []. Timeline dips 66 D -> 61 D in hold cycles 10-12 (the T2.12
    edge-host-rate warn from the journal burst after the single snapshot) and
    goes back to 66 D; the grade stays D, so it is not a flap here.
- Why no `NERVE_EDGE_HOSTS=0` pin (unlike cortex-staging-fold, D-293-3): on
  the folded main-staging nerve the same dip crosses a grade line (C->D->C)
  and counts as a flap; on this lineage it does not, so the edge sensor stays
  covered. When the two lineages meet, keep staging-fold's entries and
  expected files (they are re-blessed there for the folded nerve anyway).

## Tests (box, 2026-10-27 CET +01:00)

| Gate | Result |
|---|---|
| `replay_corpus` (box, live root present) | 4/4 pass (was 3/1) |
| `replay_corpus` with `NERVE_CAPTURE_ROOT=/nonexistent` (CI view) | 4/4 |
| `REPLAY_CORPUS_REQUIRE_LIVE=1` | 4/4, 10 replayed, 0 skipped |
| `cargo xtask replay-corpus --require-live` | ok |
| `cortex_replay` golden | 5/5 |
| `openmind-nerve` all | 28 suites 235 pass 0 fail 1 ign |
| `gaia-cortex` all | 13 suites 85 pass 0 fail 0 ign |
| workspace (excl openmind-sandbox, cgroup-detect) | 242 suites 1196 pass 0 fail 4 ign (base 1195/1/4) |
| `cargo fmt --check`, `clippy --workspace --all-targets -D warnings` | ok, 0 warnings |
| `scripts/no-python.sh` | ok |

Cross targets: no Rust code changed (test data only), so musl / android /
armv7 builds are unaffected and were not re-run.

## Real-life test (release binary + live VPS, read-only)

`GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_replay_corpus`
22:55:07 - 22:56:56 CET: 3/3 pass.

- Real `openmind-nerve replay DIR --verdict` on every corpus capture: 10 of 10
  match the committed expected verdicts (11-39 ms each), incl. the two new
  ones (replay200 76 C / 66 D, replay207 66 D / 66 D, flaps 0).
- Flap control: hermes-rs fail/recover/fail -> exit 3 with the flap named;
  fail once -> exit 0.
- Fresh read-only VPS capture (3x GET :8670/state, status.json, ss, 30 min
  journal) replayed: final 64 D held 64 D, incidents {edge, galion-one},
  flaps []; live nerve cycle 26 says 64 D with the same incidents {edge,
  galion-one}; live-only incidents [].
- VPS read-only probes 22:55 CET: nerve :8670 /state 200 (incident
  inc-865945dd galion-one-mouth stuck loading 4.2 h still open);
  `gaia-cortex.service` active (event-driven, no HTTP listener: :8672 not
  bound, so the old `:8672/healthz` probe now gives no answer - expected, not
  a fault of this branch). VPS load avg ~14-18.

Temp dirs removed by the tests; no process left running.
