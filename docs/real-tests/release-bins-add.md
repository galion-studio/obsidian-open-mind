<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: release-bins-add (wave-4 slot 337, D-325-2)

Written 2026-10-28 01:40 CET (+01:00) Europe/Zagreb on `swarm/release-bins-add`. Box only, x86_64
musl, `-j 2`, own target dirs, ports 22070-22079. **No deploy**; the VPS was only probed read-only.

## What changed

- Base `swarm/release-repro-unify` 6ffb2570 (slot 325, D-312-4 fix) had no gaia-cortex and no
  galion-console-serve, so two merges bring them in (details: docs/merges/release-bins-add.md):
  `swarm/cortex-staging-land-check` 186cd61c (slot 317: main-staging 38a07700 tree +
  cortex-staging-fold 77df80f9) and `swarm/console-v2-admin-fold` c7cebb28 (slot 331: console-v2
  + admin lockout fix dfa120c6; `galion-console-serve` is the second binary of `console-static`).
- `crates/galion-feature-pin/tests/common.rs`: RELEASE_BINS is now 14 `ReleaseBin` rows (13
  packages) with per-bin features and size budget. gaia-cortex carries `llm-advisor` (live unit is
  `gaia-cortex-73583ab-llm`); `p_args` adds `--features gaia-cortex/llm-advisor` only when
  gaia-cortex is selected. Budgets equal `scripts/size-budget.sh` (2 MiB; hermes-gateway 2.5 MiB).
- `release_feature_unify.rs` (normal `cargo test`, cargo tree only): new partial sets
  `gaia-cortex + openmind-nerve` and `console-static + gaia-cortex + hermes-gateway`; new guards
  `release_bins_are_real_binaries_and_features` (each row is a real `[[bin]]` / declared feature)
  and `release_budgets_match_size_budget_script`.
- `real_release_repro.rs` (GALION_REAL=1): full set of 13 packages, every package alone, the
  nerve+gateway pair, the gaia-cortex+nerve pair and the console recipe
  `-p console-static --bin galion-console-serve`; plus stripped + size budget on every full-set
  binary (ELF section headers read in Rust).
- `real_release_smoke.rs` (GALION_REAL=1, new): starts the two new release binaries from the
  full-set output on 22070-22079 and probes them over HTTP.

## Commands

```sh
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/release-bins-add
cargo-slot test -j 2 -p galion-feature-pin --test release_feature_unify
GALION_REAL=1 GALION_REPRO_DIR=/workspace/swarm/target/release-bins-add-work \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_repro -- --nocapture
scripts/size-budget.sh --check-dir /workspace/swarm/target/release-bins-add-work/out/x86_64-unknown-linux-musl/full
GALION_REAL=1 GALION_RELEASE_BINS_DIR=/workspace/swarm/target/release-bins-add-work/out/x86_64-unknown-linux-musl/full \
  cargo-slot test -j 2 -p galion-feature-pin --test real_release_smoke -- --nocapture
```

Provenance env of the nested builds: GALION_BUILD_COMMIT=release-bins-add,
GALION_BUILD_TIME=2026-10-28T01:30:00+01:00, SOURCE_DATE_EPOCH=1793147400, `--locked --offline`.

## Results

| check | time (CET +01:00) | result |
|---|---|---|
| feature guard (x86_64 + aarch64 musl, 13 solo + 5 partial sets) | 01:17 | 5/5 tests pass, 0 mismatches |
| feature guard negative control (solo built without `--features`) | 01:32 | **fails** as expected: 2 mismatches (gaia-cortex, both targets) |
| byte proof, first try | 01:18-01:20 | aborted: box disk 100 % full (cc `No space left on device`); slot 333 freed caches of finished slots at 01:22 |
| byte proof | 01:23-01:30 | **19 compared, 0 differ**, 0 over budget / unstripped; full set 75.1 s, test 375.0 s |
| size-budget.sh --check-dir on full set | 01:30 | ok (14 binaries) |
| smoke on release bytes | 01:31 | **14 checks pass** (below); ports 22070-22079 free before and after |
| VPS read-only probe | 01:19 | below; nothing changed |

Byte proof rows (sha256 prefix, solo == full):

```
solo -p openmind-nerve      openmind-nerve         2020224 B 1cf698662ea3b3d2 IDENTICAL
solo -p hermes-gateway      hermes-gateway         2202384 B 50ffbceec01f0717 IDENTICAL
solo -p openmind-memory     openmind-memory        1731312 B 9f2dd453d6b9498b IDENTICAL
solo -p openmind-queue      openmind-queue          897856 B 5ccf8fe3c970940e IDENTICAL
solo -p galion-one          galion-one              963904 B 29d875fb0af72c05 IDENTICAL
solo -p galion-health       galion-health           557856 B 9daec765489610f5 IDENTICAL
solo -p galion-desk         galion-desk             766752 B a09a8cc5b7fc5fa3 IDENTICAL
solo -p link-watch          link-watch              791248 B fac59b0babaf3416 IDENTICAL
solo -p tuner               tuner                   488224 B ceffe1939feeaa37 IDENTICAL
solo -p console-static      galion-console-static  1024896 B b3a90b376f5c332d IDENTICAL
solo -p console-static      galion-console-serve   1024896 B c512ed53a15037d6 IDENTICAL
solo -p galion-backup       galion-backup          1415680 B 72b2876f4310b67c IDENTICAL
solo -p desk-hermes         desk-hermes            1333792 B bca7e377c004475f IDENTICAL
solo -p gaia-cortex         gaia-cortex            1155872 B 461be740717af0d3 IDENTICAL
pair nerve+gateway          openmind-nerve / hermes-gateway                     IDENTICAL x2
pair gaia-cortex+nerve      gaia-cortex / openmind-nerve                        IDENTICAL x2
bin -p console-static --bin galion-console-serve                                IDENTICAL
```

Full-set SHA-256 (x86_64-unknown-linux-musl, this tree):

```
bca7e377c004475fb3eeff6e29af2ad5d0479118321bcbda147c2bf95fde3aed  desk-hermes  1333792
461be740717af0d3122ea6f3855a37049ef882cdc20f61e583c07422f4ddbb81  gaia-cortex  1155872
72b2876f4310b67c6dc62188a470f58f8733bd87fc5a9b0e38ccff5b05a4a634  galion-backup  1415680
c512ed53a15037d69c07d497af2b69f5414db34f4e0268e7ab2ec73827caefe2  galion-console-serve  1024896
b3a90b376f5c332d69bee7df8a96889a8c39a14bf027a2129d216743e3ef62c6  galion-console-static  1024896
a09a8cc5b7fc5fa333a78d3cc599bc95a0a13e4f11738287839dceb5302b31f2  galion-desk  766752
9daec765489610f59eb609f624816720c7e0813568f6943549af3a1acac7937e  galion-health  557856
29d875fb0af72c050bd6f0222cc266ed417420754db98d21e995c61363336987  galion-one  963904
50ffbceec01f0717e61a79c561ca68cdc5f14d50ac78fa7b0d1f67b7052484a9  hermes-gateway  2202384
fac59b0babaf34168cf3b0cd6f20f04ccacc9847abe1b1a6529b06fc6f2f1ac0  link-watch  791248
9f2dd453d6b9498bcf268fed1a9511ed4da657a7794f273c9ac8e5ccbfeb95ac  openmind-memory  1731312
1cf698662ea3b3d22d05ee6aa3f38ed1eaa67d4793eea8ac1e54b8dce2a4ca3f  openmind-nerve  2020224
5ccf8fe3c970940e65035d297f6a811b07d9aa392f2d479d34ac94cac1a00574  openmind-queue  897856
ceffe1939feeaa37e73397c9ae4dbf2b8341d27a6a537a67749f462f60584f65  tuner  488224
```

Versus slot 325 (6ffb2570): galion-health, openmind-queue and galion-backup are byte-identical.
The others differ, partly because GALION_BUILD_COMMIT differs (`release-bins-add` vs
`release-repro-unify`) and partly from the merged code; size changes: openmind-nerve 1930048 ->
2020224 (cortex-lineage nerve, now 76928 B under the 2 MiB cap), hermes-gateway 2198288 -> 2202384,
openmind-memory 1727216 -> 1731312, tuner 480032 -> 488224, galion-console-static 938784 ->
1024896 (console-v2).

Smoke (release bytes, 01:31):

```
gaia-cortex 1155872 B: llm-advisor transport present   (curl `--proto =https,http` literal in the bytes)
gaia-cortex --version -> gaia-cortex 0.1.0
gaia-cortex :22070/health -> 200 {"ok":true,"service":"gaia-cortex",...,"mode":"dry-run","llm_enabled":false,"nerve":true,...}
gaia-cortex :22070/stats /incidents /decisions /alerts /escalations -> 200 (5)
gaia-cortex :22070/nope -> 404
galion-console-serve :22071/ -> 302          (live :8080 argv: --dist --ui rust --local-api --pulse --quiet)
galion-console-serve :22071/api/v1/health -> 200 {"status":"healthy",...,"mode":"local-api"}
galion-console-serve :22071/api/v1/ping -> 200
galion-console-serve :22071/api/v1/session -> 401 {"error":"phone required"}
galion-console-serve :22071/../etc/passwd -> 404
```

VPS read-only (01:19 CET): gaia-cortex active on `gaia-cortex-73583ab-llm` (1136200 B), :8674
/health 200 `mode live, llm_enabled true, hermes_handoff true, incidents_open 2`; console and
console-rust-primary active on `galion-console-serve-2d846fb8` (949568 B); :8080 /api/v1/health
200, /api/v1/ping 200, /api/v1/session 401 (same codes as the box smoke); nerve :8670 /health 200,
/state 200 (open alert inc-865945dd galion-one mouth stuck 7.0 h). Live cortex is the source of
truth until a new build is deployed; nothing was deployed.

## Gates

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets` (excl. openmind-sandbox,
  cgroup-detect) `-D warnings` rc 0; gaia-cortex `--features llm-advisor` clippy rc 0, tests
  13 suites 85 passed.
- Workspace tests (excl. openmind-sandbox, cgroup-detect, hermes-tools, `--no-fail-fast`, 01:33-01:36):
  418 suites, 2201 passed, 1 failed, 18 ignored. The failure was
  `console-ui port2::tests::fallbacks_on_this_base` (merge semantic: console-v2 made /arena and
  /bank Rust-first, so the SPA fallback list is now empty); fixed, then console-ui + console-static
  34 suites 249 passed 0 failed 8 ignored.
- aarch64 musl release build (rust-lld): gaia-cortex (llm) 980240 B, galion-console-serve 867192 B,
  galion-console-static 867192 B. armv7 musl and aarch64-android `cargo check` of gaia-cortex (llm),
  console-static, galion-feature-pin ok. `scripts/no-python.sh` ok; SPDX header on all 108 added
  text files.

## Not run

- aarch64 musl byte proof: the full set needs a C cross compiler for bundled sqlite (D-318-3 /
  D-325-3, slot 338). The aarch64 feature graph is covered by the guard.
- Sandbox tests (sandbox_vps_only) were not run; nothing here touches the sandbox.
