<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: nerve-size-headroom (wave-4 slot 348, D-337-3)

Written 2026-10-28 03:45 CET (+01:00) Europe/Zagreb on `swarm/nerve-size-headroom`, base
`swarm/release-bins-add` fc481390. Box only. Nothing deployed, no VPS touched, no live change.

## Problem

D-337-3: release `openmind-nerve` (x86_64 musl, full release set) was 2,020,224 B, only 76,928 B
under the 2 MiB cap of `scripts/size-budget.sh`. The next nerve growth would fail the budget.

## What changed

1. `crates/openmind-nerve/build.rs` (new): on linux + musl, x86_64 or aarch64, and only when the
   resolved linker is lld, adds two linker size flags to this package's binaries
   (`cargo:rustc-link-arg-bins`):
   - `-zpack-relative-relocs` (RELR): the static-pie x86_64 binary carried 1,662 RELA entries
     (39,888 B of `.rela.dyn`); RELR packs them into 600 B. musl 1.2.5 (Rust musl targets) applies
     DT_RELR in its static-pie start code. aarch64 musl is non-PIE static (ET_EXEC, no relocations),
     so RELR is a no-op there.
   - `--icf=all`: lld folds byte-identical functions (Rust marks functions `unnamed_addr`, so
     address identity is not guaranteed by the language).
   - `NERVE_NO_SIZE_LINK_ARGS=1` switches both off (comparison builds).
2. `.cargo/config.toml`: `[target.x86_64-unknown-linux-musl] linker = "rust-lld"` (the same line as
   slot 329 / D-329-2, owner "yes"; CI `self-eval.yml` and `scripts/ucpu-gate.sh` already export it).
   Effect on the other 13 release bins: each 0.1-1.2 % smaller with lld, all still byte-identical
   solo vs full set (below).
3. `crates/openmind-nerve/tests/real_size_headroom.rs` (new, `GALION_REAL=1`): checks release bytes
   (static, stripped, machine, x86_64 >= 128 KiB headroom, DT_RELR present, < 64 RELA left) and
   smokes each binary on ports 22180-22189.
4. `crates/galion-feature-pin/tests/real_release_smoke.rs`: `GALION_SMOKE_PORT_BASE` knob (default
   22070, unchanged) so the smoke can run on another slot's port range.

Slot 329's other trims do not apply: openmind-nerve is pure Rust (no bundled SQLite, so no C
no-unwind CFLAGS or SQLite OMIT flags), and feature pruning found nothing worth it: symbol sizes of
an unstripped trimmed build show chrono-tz at 33 B (LTO keeps only Europe/Zagreb), serde_json 66 KB,
semver 5 KB, tunnel-lint 5 KB; the bulk is nerve's own code (~815 KB) and std (~396 KB, incl. the
backtrace symbolizer, which needs build-std to drop). `--icf=safe` saved 0 B (no addrsig with LTO).

## Sizes (x86_64-unknown-linux-musl unless noted)

| build | bytes | headroom under 2 MiB | sha256 prefix |
|---|---:|---:|---|
| base fc481390 (cc / GNU ld 2.44), = slot 337 full-set nerve | 2,020,224 | 76,928 | 1cf698662ea3b3d2 |
| base + rust-lld only (`NERVE_NO_SIZE_LINK_ARGS=1`) | 2,017,768 | 79,384 | |
| + RELR | 1,977,800 | 119,352 | |
| + RELR + ICF (this branch) | **1,957,160** | **139,992** | b2e02a262bac0e05 |
| aarch64 musl, lld, no flags | 1,666,712 | 430,440 | |
| aarch64 musl, this branch (ICF) | 1,650,456 | 446,696 | f9bc9e1c332c475b |

Saved on x86_64: 63,064 B (RELR ~40 KB, ICF ~21 KB, lld ~2 KB).

## Runs (all 2026-10-28, CET +01:00)

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/nerve-size-headroom
cargo-slot build --release -j 2 --locked --target x86_64-unknown-linux-musl  -p openmind-nerve
cargo-slot build --release -j 2 --locked --target aarch64-unknown-linux-musl -p openmind-nerve
GALION_REAL=1 NERVE_HEADROOM_X86=$T/x86_64-unknown-linux-musl/release/openmind-nerve \
  NERVE_HEADROOM_A64=$T/aarch64-unknown-linux-musl/release/openmind-nerve \
  NERVE_HEADROOM_BASE=<fc481390 build> \
  cargo-slot test -j 2 -p openmind-nerve --test real_size_headroom -- --nocapture
GALION_REAL=1 GALION_REPRO_DIR=/workspace/swarm/target/nerve-size-headroom-work \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
scripts/size-budget.sh --check-dir /workspace/swarm/target/nerve-size-headroom-work/out/x86_64-unknown-linux-musl/full
GALION_REAL=1 GALION_SMOKE_PORT_BASE=22180 GALION_RELEASE_BINS_DIR=<work>/out/x86_64-unknown-linux-musl/full \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_smoke -- --nocapture
```

| check | time | result |
|---|---|---|
| real_size_headroom (x86_64 + aarch64 under qemu + base) | 03:33 | PASS: x86_64 139,992 B headroom, DT_RELR, 0 RELA; 3 smokes on :22180/:22181/:22182, each /health cycle>=1, POST /signal 202, bad 400, stub healthy, dead port failing, signal in /api/v1/signals; same keys and system verdict as base |
| negative control: base fc481390 bytes as NERVE_HEADROOM_X86 | 03:34 | FAIL as required ("2020224 B leaves less than 131072 B headroom") |
| negative control: lld build without flags | 03:34 | FAIL as required (79,384 B headroom, no RELR, 1,689 RELA) |
| real_release_repro (14 bins, every package solo, 2 pairs, console --bin) | 03:41 | **19 compared, 0 differ, 0 over budget / unstripped** (383 s) |
| size-budget.sh --check-dir full set | 03:42 | ok (14 binaries), nerve 1,957,160 |
| real_release_smoke on full set, ports 22180-22189 | 03:42 | 14 checks passed (gaia-cortex llm transport, 7 cortex routes, console 302/200/200/401/404) |
| lld-linked C (SQLite) bins, full set | 03:43 | openmind-memory :22184 /health 200, /capture l0_recorded 2, FTS5 /search/conversations 2 hits; galion-backup snapshot ok + verify integrity=ok tables=15; `--version` ok on 11 bins (console has no --version; hermes-gateway refuses start without key, as designed) |
| openmind-nerve tests (host) | 03:37 | 47 suites, 382 passed / 0 failed / 3 ignored |
| clippy -D warnings, openmind-nerve + galion-feature-pin all targets | 03:38 | ok |
| cargo fmt --all --check | 03:38 | ok |
| cargo check armv7-unknown-linux-musleabihf, aarch64-linux-android (nerve) | 03:45 | ok (build.rs no-op there) |
| no-python.sh / spdx-scan.sh | 03:43 | ok / 810 scanned, 0 missing |
| workspace tests (host; excl openmind-sandbox, cgroup-detect, hermes-tools), --no-fail-fast | 03:50 | **419 suites, 2203 passed / 0 failed / 18 ignored**. A first run at 03:46 hit a transient desk-hermes `mindpack` fixture failure (SQLite "readonly database" / "disk I/O error" on the shared fixture home); it passed alone (9/0) and in the full re-run. Host builds do not use either change (musl-only config, build.rs no-op on gnu). |

Full-set SHA-256 (x86_64 musl, provenance env of real_release_repro: commit `release-bins-add`,
time 2026-10-28T01:30:00+01:00), sizes vs slot 337:

| bin | slot 337 | this branch |
|---|---:|---:|
| openmind-nerve | 2,020,224 | 1,957,160 |
| hermes-gateway | 2,202,384 | 2,198,160 |
| openmind-memory | 1,731,312 | 1,728,080 |
| openmind-queue | 897,856 | 894,472 |
| galion-one | 963,904 | 960,648 |
| galion-health | 557,856 | 557,256 |
| galion-desk | 766,752 | 757,704 |
| link-watch | 791,248 | 785,336 |
| tuner | 488,224 | 481,096 |
| galion-console-static | 1,024,896 | 1,019,144 |
| galion-console-serve | 1,024,896 | 1,019,176 |
| galion-backup | 1,415,680 | 1,414,248 |
| desk-hermes | 1,333,792 | 1,329,128 |
| gaia-cortex | 1,155,872 | 1,152,872 |

Not done: aarch64 full-set byte proof (D-337-4, C cross compiler; nerve alone is pure Rust and was
built and run under qemu). No VPS run: nothing here touches a live unit; deploy stays with the
cortex/nerve pair runbook (slot 357).
