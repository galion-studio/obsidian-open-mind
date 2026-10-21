<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Security S2.4: Miri on the pure-logic crates (slot 190, sec-miri-pure)

Base `integration-4` 42de672. Box only. Runs 2026-10-21 14:36-15:16 +02:00.
Toolchain: `rustc 1.101.0-nightly (07e526db9 2026-10-19)`, `miri 0.1.0 (07e526db9d 2026-10-19)`.
Crates: `galion-common`, `hades-gate` (token, safety, config in memory; ledger is
file-backed, see below), `openmind-schema`. Both `galion-common` and `hades-gate`
are `#![forbid(unsafe_code)]`; `openmind-schema` has no `unsafe`. Miri therefore
mostly checks the dependencies they pull in under test (ed25519-dalek,
curve25519-dalek serial backend, sha2, zeroize, serde, serde_json, toml,
zagreb-time) and std.

## Result: 0 UB findings

No undefined behaviour, no data race, no leak (outside the documented
never-exiting serve thread) was reported in any pass. There is no file:line to
report.

| Pass | MIRIFLAGS | Test args | Passed | Failed | Ignored |
|---|---|---|---|---|---|
| 1 isolation (Stacked Borrows) | (none) | | 60 | 0 | 36 (FS/net/clock/real, all with a reason) |
| 2 FS + net | `-Zmiri-disable-isolation` (+ `-Zmiri-ignore-leaks` for galion-common) | `--include-ignored` (approval: `--skip concurrent_consumers_approve_exactly_once`) | 81 | 0 | 1 skipped |
| 2b live captures | `-Zmiri-disable-isolation`, `GALION_MIRI_CAPTURE=<dir>` | `--nocapture` | 2 | 0 | 0 |
| 3 Tree Borrows | `-Zmiri-tree-borrows` | | 28 | 0 | 9 |

Per target (pass 1 / pass 2):

| Crate / target | Pass 1 | Pass 2 |
|---|---|---|
| galion-common lib | 21 / 0 / 1 (386.8 s Miri clock) | 22 / 0 / 0 |
| galion-common http_fuzz | 4 / 0 / 2 | 6 / 0 / 0 (real TCP: garbage server + slowloris) |
| galion-common nist | 3 / 0 / 0 (FIPS 180-2 B.3 one million `a`, 421 s wall) | - |
| galion-common time_vs_chrono | 3 / 0 / 0 (1108 s wall) | - |
| galion-common real_common, real_hammer, real_migrate, real_version | 0 / 0 / 6 | not run (process spawn / release builds) |
| hades-gate lib (token, safety, config) | 17 / 0 / 0 | 17 / 0 / 0 |
| hades-gate approval (ledger + gate) | 0 / 0 / 12 | 11 / 0 / 0, 1 skipped |
| hades-gate decide | 4 / 0 / 1 | 5 / 0 / 0 |
| hades-gate journal | 0 / 0 / 5 | 5 / 0 / 0 |
| hades-gate redteam (82-attack corpus, in-process) | 3 / 0 / 0 | 3 / 0 / 0 |
| hades-gate simd_free | 1 / 0 / 0 | 1 / 0 / 0 |
| hades-gate real_cli, real_redteam | 0 / 0 / 2 | not run (spawn release binary) |
| openmind-schema lib | 4 / 0 / 7 | 11 / 0 / 0 |
| galion-common real_miri_capture (new) | - | 1 / 0 / 0 (7 captures, 27 stamps) |
| openmind-schema real_miri_capture (new) | - | 1 / 0 / 0 (7 captures, 6 JSON bodies, 1 nerve event) |

Tree Borrows: openmind-schema lib 4/0/7, hades-gate lib 17/0, hades-gate
redteam 3/0, galion-common http_fuzz 4/0/2.

## Miri limits hit (interpreter limits, not UB)

1. **Contended blocking `flock`** - `hades-gate/src/ledger.rs:93`
   (`Ledger::consume` -> `File::lock`), reached from
   `tests/approval.rs:304` `concurrent_consumers_approve_exactly_once`:
   `unsupported operation: blocking flock is not currently supported`. The
   test is `cfg_attr(miri, ignore)` with that reason and `--skip`ped in pass 2;
   the native run covers it (passes). Uncontended `flock` in the other ledger
   tests works under Miri.
2. **Directory fsync** - `Ledger::open` syncs the parent directory only when it
   creates the ledger file; Miri cannot fsync a directory fd (seen by this
   slot's first run, before 14:27 +02:00; not re-measured after the workaround). Under
   `cfg(miri)` only, `tests/approval.rs` creates the empty ledger first, so
   scan, flock, append and file fsync still run.
3. **`/proc` reads** - `decide` reads `/proc/self/task`; Miri warns that `/proc`
   bypasses the abstract machine (warning only, test passes).
4. **Speed** - Miri is ~100-1000x slower. Test-only, `cfg(miri)`-only cuts:
   `http_fuzz` default iterations /100 (min 20; `GALION_FUZZ_ITERS` still
   overrides), `time_vs_chrono::every_switch_1996_to_2100` sweeps 1996, 2022,
   2048, 2074, 2026, 2027 instead of every year. Native builds are unchanged.

## What changed (test-only, no runtime behaviour change)

- `#[cfg_attr(miri, ignore = "<reason>")]` on every test that needs the FS,
  network, real-time clock or process spawn (reasons say which pass runs it).
- `galion-common/src/time.rs`: the `now_rfc3339()` length check moved from
  `known_instants` into its own `now_shape` test so the pure instants run with
  isolation on.
- `openmind-schema/src/lib.rs`: new `in_memory_config_builds_the_stack` test
  (TOML parse + env overrides + provider list + serde round trip, no file) so
  the config path runs with isolation on.
- New real-data tests `crates/galion-common/tests/real_miri_capture.rs` and
  `crates/openmind-schema/tests/real_miri_capture.rs`, gated by
  `GALION_MIRI_CAPTURE` (see docs/real-tests/sec-miri-pure.md).
- No `src` logic, no dependency, no `Cargo.toml` change.

## How to run

```
export CARGO_TARGET_DIR=/workspace/swarm/target/sec-miri-pure
# pass 1
cargo-slot +nightly miri test -j 2 -p galion-common -p hades-gate -p openmind-schema
# pass 2 (FS + net)
MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" cargo-slot +nightly miri test -j 2 -p galion-common -- --include-ignored
MIRIFLAGS=-Zmiri-disable-isolation cargo-slot +nightly miri test -j 2 -p openmind-schema -- --include-ignored
MIRIFLAGS=-Zmiri-disable-isolation cargo-slot +nightly miri test -j 2 -p hades-gate -- --include-ignored --skip concurrent_consumers_approve_exactly_once
# pass 3
MIRIFLAGS=-Zmiri-tree-borrows cargo-slot +nightly miri test -j 2 -p hades-gate --lib
```

The GALION_REAL tests skip themselves without `GALION_REAL=1` in pass 2.

## Owner decisions

- D-M1: add a CI job for pass 1 (about 25 min wall on this box, mostly
  `nist` B.3 and `time_vs_chrono`) or only pass 1 minus those two (~3 min)?
- D-M2: `Ledger::open` uses a blocking `flock`; a `try_lock` + bounded retry
  would make the contended path Miri-checkable and bound how long a stuck
  holder can stall approvals. This is a behaviour change, so not done here.
