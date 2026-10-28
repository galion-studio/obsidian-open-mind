<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: relr-other-bins (wave-4 slot 368, D-348-3 / D-348-4 prep)

Written 2026-10-28 06:55 CET (+01:00) Europe/Zagreb on `swarm/relr-other-bins`, base
`swarm/nerve-size-headroom` 463f6a71. Box measurements + read-only VPS probes. Nothing deployed,
no build setting adopted: the per-bin numbers below are for the owner's decision (D-368-1).

## What was measured

The slot 348 linker trims (lld `-z pack-relative-relocs` = RELR, and `--icf=all`) applied to the
other 13 release binaries of `RELEASE_BINS` (galion-feature-pin `tests/common.rs`), full release
`-p` set exactly as `real_release_repro` builds it (same `GALION_BUILD_COMMIT=release-bins-add`,
`GALION_BUILD_TIME`, `SOURCE_DATE_EPOCH`; `gaia-cortex/llm-advisor`), on x86_64 and aarch64 musl.

Method (measurement only, nothing committed for it): three throw-away linker wrappers outside the
repo (`/workspace/swarm/tmp-368/<v>/ld.lld` = `rust-lld -flavor gnu "$@" <flags>`, removed after the
run), selected with `CARGO_TARGET_<TRIPLE>_LINKER`, so only the final link changes:

- `lld`: no extra flag (= this branch as is; x86_64 bytes equal the slot 348 table, e.g.
  openmind-memory 1,728,080, nerve sha256 b2e02a262bac0e05),
- `relr`: `-zpack-relative-relocs`,
- `relr+icf`: `-zpack-relative-relocs --icf=all`.

openmind-nerve already gets both flags from its own build.rs (slot 348), so its row is 0 in
every column. aarch64 C code (bundled SQLite) was cross-compiled with the slot 338 zig wrapper
(`/workspace/swarm/bin/aarch64-linux-musl-gcc`), so the aarch64 full set now builds on the box
(the slot 348 doc listed this as not done, D-337-4).

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/relr-other-bins
export GALION_BUILD_COMMIT=release-bins-add GALION_BUILD_TIME=2026-10-28T01:30:00+01:00 SOURCE_DATE_EPOCH=1793147400
CARGO_TARGET_<TRIPLE>_LINKER=/workspace/swarm/tmp-368/<v>/ld.lld cargo-slot build --release --locked --offline -j 2 \
  --target <triple> -p openmind-nerve -p hermes-gateway -p openmind-memory -p openmind-queue -p galion-one \
  -p galion-health -p galion-desk -p link-watch -p tuner -p console-static -p galion-backup -p desk-hermes \
  -p gaia-cortex --features gaia-cortex/llm-advisor
```

Builds 06:22-06:29 CET, each variant 62-92 s (dependencies cached, only the links change).

## Per-bin bytes: x86_64-unknown-linux-musl (static-pie, ET_DYN)

| bin | lld (now) | relr | relr+icf | Δ RELR | Δ ICF | Δ total | headroom after (cap) |
|---|---:|---:|---:|---:|---:|---:|---:|
| openmind-nerve | 1,957,160 | 1,957,160 | 1,957,160 | 0 | 0 | 0 (already) | 139,992 (2 MiB) |
| hermes-gateway | 2,198,160 | 2,131,664 | 2,113,008 | -66,496 | -18,656 | **-85,152** | 508,432 (2.5 MiB); **-15,856 vs 2 MiB** |
| openmind-memory | 1,728,080 | 1,668,560 | 1,659,504 | -59,520 | -9,056 | **-68,576** | 437,648 |
| openmind-queue | 894,472 | 867,816 | 857,992 | -26,656 | -9,824 | -36,480 | 1,239,160 |
| galion-one | 960,648 | 937,032 | 929,128 | -23,616 | -7,904 | -31,520 | 1,168,024 |
| galion-health | 557,256 | 537,544 | 535,336 | -19,712 | -2,208 | -21,920 | 1,561,816 |
| galion-desk | 757,704 | 740,072 | 734,696 | -17,632 | -5,376 | -23,008 | 1,362,456 |
| link-watch | 785,336 | 761,912 | 753,848 | -23,424 | -8,064 | -31,488 | 1,343,304 |
| tuner | 481,096 | 469,224 | 467,048 | -11,872 | -2,176 | -14,048 | 1,630,104 |
| galion-console-static | 1,019,144 | 986,472 | 982,024 | -32,672 | -4,448 | -37,120 | 1,115,128 |
| galion-backup | 1,414,248 | 1,361,992 | 1,358,568 | -52,256 | -3,424 | **-55,680** | 738,584 |
| desk-hermes | 1,329,128 | 1,287,816 | 1,283,560 | -41,312 | -4,256 | -45,568 | 813,592 |
| gaia-cortex (llm-advisor) | 1,152,872 | 1,122,536 | 1,111,400 | -30,336 | -11,136 | -41,472 | 985,752 |
| galion-console-serve | 1,019,176 | 986,472 | 982,024 | -32,704 | -4,448 | -37,152 | 1,115,128 |
| **13 other bins total** | | | | **-438,208** | **-90,976** | **-529,184** | |

Every relr+icf binary: ET_DYN with DT_RELR (readelf -d shows RELR/RELRSZ/RELRENT), stripped.

## Per-bin bytes: aarch64-unknown-linux-musl (static, non-PIE ET_EXEC)

The Rust aarch64 musl target links non-PIE static here (ET_EXEC, no `.rela.dyn` RELATIVE
entries), so RELR is a no-op (0 B on every bin, no DT_RELR). Only ICF counts:

| bin | lld (now) | relr+icf | Δ ICF |
|---|---:|---:|---:|
| openmind-nerve | 1,650,456 | 1,650,456 | 0 (already) |
| hermes-gateway | 2,048,992 | 2,031,472 | -17,520 |
| openmind-memory | 1,695,432 | 1,685,368 | -10,064 |
| openmind-queue | 763,096 | 754,712 | -8,384 |
| galion-one | 845,048 | 838,760 | -6,288 |
| galion-health | 496,888 | 494,760 | -2,128 |
| galion-desk | 665,368 | 660,328 | -5,040 |
| link-watch | 678,824 | 672,936 | -5,888 |
| tuner | 432,544 | 430,480 | -2,064 |
| galion-console-static | 867,192 | 862,968 | -4,224 |
| galion-backup | 1,427,984 | 1,423,488 | -4,496 |
| desk-hermes | 1,350,576 | 1,345,520 | -5,056 |
| gaia-cortex (llm-advisor) | 980,224 | 970,384 | -9,840 |
| galion-console-serve | 867,192 | 862,968 | -4,224 |
| **13 other bins total** | | | **-85,216** |

## Real runs on the trimmed bytes (all 2026-10-28 CET +01:00)

| check | time | result |
|---|---|---|
| `--version` / argv check of all 14 aarch64 relr+icf bins under qemu-aarch64 | 06:31 | 11 print version (with provenance); console-static/serve "unknown argument --version", gateway refuses without key, backup prints usage: same as slot 348 x86_64 behaviour |
| real_release_smoke x86_64 relr+icf, `GALION_SMOKE_PORT_BASE=22380` | 06:31 | **14 checks passed** (gaia-cortex llm transport + --version + 7 routes, console 302/200/200/401/404) |
| real_release_smoke aarch64 lld and relr+icf under qemu (new `GALION_SMOKE_RUNNER`) | 06:32 | **14 checks passed** each |
| real_release_smoke x86_64 relr+icf, runner unset (knob default unchanged) | 06:32 | 14 checks passed |
| openmind-memory real HTTP + galion-backup on SQLite, 4 runs: x86_64 lld :22383, x86_64 relr+icf :22382, aarch64 lld :22384, aarch64 relr+icf :22385 (qemu) | 06:32 | identical on all 4: /health 200, 2x POST /capture 200 `l0_recorded 2`, POST /search/conversations 200 total 2, clean stop on SIGTERM; galion-backup snapshot ok + verify `integrity=ok tables=15` + schema user_version=4 |
| tests of all 13 release packages built for x86_64 musl **with the relr+icf linker** (test binaries are static-pie too), `--release --no-fail-fast` | 06:32-06:43 | **150 suites, 923 passed / 1 failed / 14 ignored**. The 1 failure is F-368-1 below, and it fails the same way without the flags |
| F-368-1 control: `openmind-nerve --lib` x86_64 musl release, plain lld vs relr+icf, 2 runs each | 06:49-06:52 | lld: FAIL (wakes 25, 10), relr+icf: FAIL (wakes 20, 11); same test alone 5/5 + 5/5 pass for both; host gnu debug `--lib` 229/0 |
| size_headroom_guard (new) on the 4 full sets | 06:30 | 14/14 ok at the default 128 KiB on all four (tightest: nerve 136 KiB) |
| size_headroom_guard, `GALION_HEADROOM_WARN_KIB=400` | 06:31 | 13 ok, 1 WARN (nerve 136 KiB < 400 KiB), test passes |
| same + `GALION_HEADROOM_STRICT=1` | 06:31 | FAILS as required ("low headroom: [openmind-nerve]") |
| negative control: copy of the set with +500,000 B on openmind-memory and tuner removed | 06:31 | FAILS as required (memory headroom -62,352, tuner missing) |
| negative control: `GALION_HEADROOM_WARN_KIB=12k` | 06:31 | FAILS with a clear message |

Ports used: 22380 (cortex), 22381 (console), 22382-22385 (memory), 22388 (console pulse),
22389 (dead nerve port). After the runs: no listener on 22380-22389, no process left.

## Live VPS cross-check (read-only, 2026-10-28 06:52-06:53 CET)

`ssh slavik-vps`: `ls -l /opt/galion/rust/bin`, `readelf -h/-d/-r` on the binaries the live units
run, `curl` GET of nerve and cortex health. No write, no unit touched.

| live binary (unit) | bytes | type / DT_RELR | R_X86_64_RELATIVE entries | RELR would save about (24 B each) |
|---|---:|---|---:|---:|
| openmind-nerve-9d1a4f3 (galion-nerve) | 1,458,760 | DYN / none | 1,441 | ~34.6 KB |
| gaia-cortex-73583ab-llm (gaia-cortex) | 1,136,200 | DYN / none | 1,262 | ~30.3 KB |
| hermes-gateway-b7fbbc78 (hermes-rs) | 2,732,248 | DYN / none | 2,536 | ~60.9 KB |
| openmind-memory-aa77864 (openmind-memory) | 1,702,640 | DYN / none | 2,469 | ~59.3 KB |
| galion-console-serve-2d846fb8 (:8080) | 949,568 | DYN / none | 1,356 | ~32.5 KB |

So the live bytes carry the same ~24 B/relocation tax the box measured. Live state at 06:52:59:
nerve `/health` cycle 60 status ok (system degraded, score 44), `/state` 200 40,687 B; cortex
`:8674/health` mode live, nerve true, incidents_open 4; galion-nerve, gaia-cortex, hermes-rs,
openmind-memory active. The live hermes-gateway (2.73 MB) is over 2 MiB (F-353-1); this branch's
gateway with RELR+ICF would be 2,113,008 B: still 15,856 B over a 2 MiB cap (the script's cap
for it is 2.5 MiB), so RELR+ICF alone does not close F-353-1; together with slot 353 (16bfc28d)
it likely does (not measured here).

## Findings

- **F-368-1** `openmind-nerve` `evt::runtime::tests::idle_is_quiet_and_burst_escalates_once`
  fails in the full `--lib` run of an x86_64 musl `--release` build (`wakes 10..25`, assert
  `2..=8`), with or without RELR/ICF; it passes alone and on host gnu. Timing-sensitive test
  (journal events not drained in one batch under load). Not changed here (nerve scope); owner/
  nerve slot: widen the bound or drive the clock explicitly.
- **F-368-2** aarch64 musl release bins are non-PIE ET_EXEC, so RELR (and ASLR of the image)
  does not apply there; x86_64 musl is static-pie. Only matters if aarch64 hosts are added.
- **F-368-3** The aarch64 full release set builds on the box now (zig C cross compiler from
  slot 338), closing the slot 348 "not done: aarch64 full-set byte proof" note for sizes.
