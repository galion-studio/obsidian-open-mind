<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Release pair check: release-artifacts-refresh on top of release-repro-unify (wave-4 slot 339)

Written 2026-10-28 02:45 CET (+01:00) Europe/Zagreb. Branch `swarm/release-pair-check`, throwaway
combine for owner decision D-325-1. **No main-staging update, no deploy.** VPS read-only only.

## What the branch is

| Commit | What |
|---|---|
| 98466ab9 | main-staging (staging-ff-land) |
| 3f015986 | tree snapshot of main-staging 38a07700 (PR #13 merge) |
| 9eaeadd1, 5fad3c2f, 6ffb2570 | slot 325 `release-repro-unify` (feature pins, galion-jsonout, galion-feature-pin, docs) |
| 124a2124 | slot 312 `release-artifacts-refresh` 7091f76b cherry-picked (same tree change, same message) |
| this commit | port base env for the artifacts test, this doc, pointers in the slot 312 docs |

## Conflicts

- **Code: none.** The two branches touch disjoint files (325: Cargo manifests, galion-common,
  galion-jsonout, galion-feature-pin, gaia ownership; 312: `real_release_artifacts.rs`, docs,
  `scripts/size-budget.sh`). The cherry-pick applied cleanly.
- The D-325-1 rebase note ("switch `features = ["serde-json"]` on galion-common to galion-jsonout")
  does **not** apply to slot 312: it adds no dependency and no feature.
- **Docs: one stale claim.** Slot 312 says the release `-p` set matters (D-312-4). On this tree it
  does not (byte proof below). Both slot 312 docs now carry a pointer to this file; the recipe
  `-p` list stays valid, it is just no longer required.
- Slot 312 SHA/size table (zig cc, 38a07700) and slot 325 table (box `x86_64-linux-musl-gcc`) are not
  comparable: different C compiler for bundled SQLite. No zig on the box today, so this check also
  uses musl-gcc; its sizes match slot 325 to the byte.
- Test port clash risk: `real_release_artifacts` hard-coded 21820-21829 (slot 312 range). Added
  `GALION_RELEASE_PORT_BASE` (default 21820, behaviour unchanged); this run used 22090.

## Results (box x86_64, VPS read-only)

| Check | Time (CET +01:00) | Result |
|---|---|---|
| 12-bin release matrix x86_64 musl, `--locked -j 2`, commit 124a2124, SOURCE_DATE_EPOCH 1793151000 | 02:26-02:31 | built in 1 m 12 s |
| `size-budget.sh --check-dir` x86_64 (12 + cortex 77df80f + console-serve 2d846fb from slot 312) | 02:31 | **ok (14)**; hermes-gateway-124a212 2,198,288 B under its 2.5 MiB cap |
| same with the 6ffb2570 `size-budget.sh` (negative control) | 02:31 | **FAIL**: tagged hermes-gateway checked against 2 MiB, so the slot 312 script fix is needed on the 325 base |
| `real_release_artifacts` on the release bytes, ports 22090/22091/22099 | 02:31:27-02:31:34 | **21 checks passed**, 1/1 test; ports free after, no `/tmp/r312-*` left |
| `release_feature_unify` (cargo tree guard, both targets) | 02:31-02:33 | 3/3 pass |
| `real_release_repro` byte proof x86_64 (GALION_REAL=1) | 02:31:40-02:36:55 | **14 compared, 0 differ** (12 solo + nerve/gateway pair); full-set SHAs equal the slot 325 table for all 12 bins, so the slot 312 commit changes no release byte |
| aarch64 musl, 8 bins without C (nerve, queue, one, health, desk, link-watch, tuner, console-static) | 02:33-02:34 | built 43.5 s; all AArch64, no PT_INTERP; size-budget ok (8); 5 equal slot 312 sizes, galion-one/link-watch/tuner differ only by build-info length |
| aarch64 solo vs 8-set: openmind-nerve, galion-one, console-static | 02:38-02:39 | **3/3 IDENTICAL** (first aarch64 byte evidence for D-325-3; SQLite bins still need a C cross compiler) |
| VPS read-only probe | 02:27:03 | nerve :8670 /health 200, /state 200 (37,049 B); hermes :8642 200; console :8080 + :18080 api health 200; 11 units active; live ExecStart unchanged (nerve 9d1a4f3, hermes-gateway b7fbbc78, memory aa77864, backup 98466ab); loadavg 18.1 |
| Gates | see `docs/real-tests/release-pair-check.md` | |

## x86_64 SHA-256 (release bytes, commit 124a2124)

```
54f5047f6e6b2b8e49d0ab8cd973985d1f4437d6c9e62256c968ab76abaa9d03  desk-hermes-124a212
72b2876f4310b67c6dc62188a470f58f8733bd87fc5a9b0e38ccff5b05a4a634  galion-backup-124a212
39e4ba2b1c34645b4e4fecbc275d02a6d7029fe3f912e00800d31845458c50ac  galion-console-static-124a212
9984a48c455e6bd1f9ace3f0b269e8bef9622a2446eaa5800fc612ca8cfe0822  galion-desk-124a212
9daec765489610f59eb609f624816720c7e0813568f6943549af3a1acac7937e  galion-health-124a212
8fc1d2fbf82f88f29c5e232c9201b4c7a1ddc90913ace6b669fda98e73d826f5  galion-one-124a212
b102cad81a6cdef9efd9ba32d4b3f1105ba0e5013b72ae5c7acb30ef3463a6aa  hermes-gateway-124a212
9a68c5d7eaee6158afeba0e3487642b11a6b653bfff71a4b656b242f505af3e8  link-watch-124a212
a9ad182cf8ec6b8bfce3ee48b173c915ad33bb278997c4c3431231a0531f81d2  openmind-memory-124a212
890b730835d59697aa2c2ddcae9b448e271dda4c784f350c728ef14375e08fdb  openmind-nerve-124a212
5ccf8fe3c970940e65035d297f6a811b07d9aa392f2d479d34ac94cac1a00574  openmind-queue-124a212
f99f92a428d85a00e9c6430e1be15e857ddc28aa641496817680d8e21bf6a353  tuner-124a212
```

Bins that embed the build commit (one, desk, link-watch, tuner, memory, desk-hermes) differ from the
slot 325 SHAs only because the commit string differs; the byte proof above used one fixed commit.
On disk: `/workspace/swarm/artifacts/release-20261028-r339/` (SHA256SUMS per triple, logs/).

## Owner decisions

- **D-339-1** Land order for main-staging: slot 325 (6ffb2570) first, then slot 312 (7091f76b)
  on top, as on this branch. Proven clean and green here; the reverse order also works (disjoint
  files) but 312's "package set matters" text would then be wrong until 325 lands.
- **D-339-2** Release recipe: keep the 12-bin `-p` list as the single build command (simplest), but
  per-bin rebuilds are now safe for hotfixes.
- **D-339-3** C compiler for publish: zig cc (slot 312, sizes +~115 KB on hermes-gateway) vs box
  musl-gcc (this run). Pick one and pin it, or publish sizes/SHAs will not match between builders.
  aarch64 SQLite bins need zig either way (D-318-3).
- Still open: D-325-2 (cortex and console-serve into RELEASE_BINS when they land), D-312-3
  (`--version` on console-static/serve and galion-backup), D-294-1/2 size trim.
