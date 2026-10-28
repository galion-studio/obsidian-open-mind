<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: release-repro-unify (wave-4 slot 325)

Written 2026-10-27 23:10 CET (+01:00) Europe/Zagreb on `swarm/release-repro-unify` (base 3f015986,
tree identical to main-staging 38a07700). Box only, x86_64 musl, `-j 2`, own target dirs, ports
21950-21959. **No deploy**; the VPS was only probed read-only.

## What is proved

D-312-4: a solo or partial `cargo build --release -p <bin>` gave different bytes than the full
12-bin build, because cargo resolver 2 unifies features only over the selected packages. After
the fix every one of the 12 release bins built alone (fresh target dir each) is byte-identical to
the same bin from the full-set build, and so is the `openmind-nerve + hermes-gateway` pair.

## Commands

```sh
export PATH=/workspace/swarm/bin:$PATH
# fast guard (cargo tree feature sets, no build), runs in the normal workspace tests
cargo-slot test -j 2 -p galion-feature-pin --test release_feature_unify
# byte proof (about 5 min with -j 2)
GALION_REAL=1 GALION_REPRO_DIR=/workspace/swarm/target/release-repro-unify-work \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
```

The real test sets GALION_BUILD_COMMIT=release-repro-unify, GALION_BUILD_TIME=2026-10-27T23:00:00+01:00,
SOURCE_DATE_EPOCH=1793138400 and clears CARGO_*/RUSTC* from the nested cargo. C code (bundled sqlite)
was built with the box `x86_64-linux-musl-gcc` (no zig on the box), so sizes are not comparable
with the slot 312 matrix, which used zig.

## Results

| check | time (CET +01:00) | result |
|---|---|---|
| feature guard on the fix (both targets) | 22:58 | 0 mismatches, 3/3 tests pass |
| feature guard on base (negative control) | 22:59 | **fails**, 30 mismatches (15 per target): galion-common `serde-json`, indexmap `std`/`default`, rusqlite `backup`, serde_core `alloc`/`result` |
| byte proof on the fix | 23:00-23:04 | **14 compared, 0 differ** (12 solo + pair of 2); full set 47.8 s, test 260.9 s |
| byte proof on base (negative control) | 23:00-23:04 | 10 of 14 differ, rc 101; only galion-health, link-watch, galion-console-static, galion-backup were already identical |
| smoke on the fixed release bytes | 23:04 | 19 checks pass: 9 `--version`, HTTP health 200 for nerve, tuner, galion-health, queue, memory, hermes-gateway, galion-one, galion-desk, console-static; galion-backup snapshot + verify integrity=ok tables=15; ports 21950/21951/21959 free before and after |
| VPS read-only probe | 23:04 | 11 units active; nerve :8670 /health 200, /state 200; hermes :8642 200; console :8080 api health 200; nothing changed |
| workspace gates | 23:04-23:10 | fmt ok; clippy -D warnings (workspace excl. sandbox, cgroup-detect) rc 0; workspace tests (excl. sandbox, cgroup-detect, hermes-tools, --no-fail-fast) 2007 passed, 1 failed (gaia ownership_counts, needed +2 crates), 18 ignored; after the count fix gaia + gaia-core all pass |

Sizes are the same on base and fix for all 12 bins; `galion-one` from the fix equals the base
**solo** build (ff3f39c7), which shows the fix moves the full set to the minimal feature set, not
the other way round.

## Full-set SHA-256 (fix, x86_64-unknown-linux-musl)

```
46f8c3e5bac63b6c315659f028ca2fce2211432bc8e21ce7353cfb229229077e  desk-hermes  1333792
72b2876f4310b67c6dc62188a470f58f8733bd87fc5a9b0e38ccff5b05a4a634  galion-backup  1415680
39e4ba2b1c34645b4e4fecbc275d02a6d7029fe3f912e00800d31845458c50ac  galion-console-static  938784
15bc1d23973a82d48f89a51029b3e6cda5e55b0aac31c131108553345d6b7082  galion-desk  766752
9daec765489610f59eb609f624816720c7e0813568f6943549af3a1acac7937e  galion-health  557856
ff3f39c73ec9415b849bd379cd199a2e7acaa93f7c49a7082233260b85f974a3  galion-one  963904
b102cad81a6cdef9efd9ba32d4b3f1105ba0e5013b72ae5c7acb30ef3463a6aa  hermes-gateway  2198288
eb6939ea0b7cc28f967da59abc4833836fa1429010064b2326185a78a35d15f5  link-watch  791248
5437d5c6ff1eea2d3b8511586b2fca09d8678b40938ed9233ba86c27e7a1aacb  openmind-memory  1727216
890b730835d59697aa2c2ddcae9b448e271dda4c784f350c728ef14375e08fdb  openmind-nerve  1930048
5ccf8fe3c970940e65035d297f6a811b07d9aa392f2d479d34ac94cac1a00574  openmind-queue  897856
9d0db4e39d161ef385bfeb7bcdc2004327f519feedbb36988cc7fd71f37a13b1  tuner  480032
```

## Not run

- aarch64 musl byte proof: no zig / aarch64 C cross compiler on the box (D-318-3). The cargo tree
  guard covers aarch64 feature sets. Owner decision D-325-3.
- gaia-cortex and console-serve are not on this base, so they are not in RELEASE_BINS (D-325-2).
