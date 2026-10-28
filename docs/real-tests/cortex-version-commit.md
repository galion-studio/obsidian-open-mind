<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 377 cortex-version-commit: real-test record

Branch `swarm/cortex-version-commit` off cortex-nerve-pair-runbook `4bd62a6b`
(cortex code = cortex-live-merge-4 `58a12e61`). Fixes F-357-1 (cortex
`--version` showed no commit). All times Europe/Zagreb CET (+01:00),
2026-10-28. Box loopback ports 22470-22472; VPS throwaway
`/var/tmp/stamp-377-20261028094141` as `nobody` on 127.0.0.1:22470.
**No deploy**: no live file, unit, permission or service changed. No new
dependencies (std `build.rs` + existing `zagreb-time`).

## What changed

- `crates/gaia-cortex/build.rs` (std only) stamps two values at compile time:
  - commit: `GALION_BUILD_COMMIT` (7-64 hex, else the build fails) if set, else
    `git rev-parse HEAD` **only if** the git top level is this workspace (a
    `git archive` tree unpacked inside another repo does not borrow its HEAD),
    else `unknown`;
  - date: `SOURCE_DATE_EPOCH` if set, else the commit's committer date from git,
    else `unknown`. **Never the wall clock**, so one commit = one binary.
  - Re-stamps when HEAD / the branch ref / packed-refs change (rerun-if-changed
    only on paths that exist, so no rebuild on every build).
- `src/build_info.rs`: `version_line()`, `json()`; date rendered in
  Europe/Zagreb RFC 3339 via `zagreb_time::from_unix` (never `Z`).
- `gaia-cortex --version`:
  `gaia-cortex 0.1.0 (commit fd4aa614, built 2026-10-28T09:38:26+01:00, features llm-advisor)`
- `/health` gains `"commit": "<full sha>"` and
  `"build": {"commit", "date", "features"}`; all old keys unchanged.

### Build recipe (archive builds)

A `git archive` tree has no `.git`, so stamp it explicitly or it says `unknown`:

```
GALION_BUILD_COMMIT=<sha> SOURCE_DATE_EPOCH=$(git show -s --format=%ct <sha>) \
  cargo build --release --locked -p gaia-cortex --features llm-advisor --target x86_64-unknown-linux-musl
```

That build is byte-identical to a plain checkout build of the same commit.

## Reproducible-build check (unchanged)

| build | source | bytes | sha256 |
|---|---|---|---|
| base 4bd62a6b, `git archive`, llm, x86_64 musl (09:36) | no stamp code | 1,147,680 | `7ca6512b1ecb1d66eea6446498c92812796fd2d98374a6b5c24ce2e00edd49a2` = slot 341/357 candidate |
| A fd4aa614 archive dir 1 + env stamp | env | 1,147,680 | `f65eff3d9db01ec6e04dcb7f61af55b48210fdb3511a3d9b5b2cee1626a56e17` |
| B fd4aa614 archive, other dir/depth, other target dir + env | env | 1,147,680 | `f65eff3d…6e17` (= A) |
| C fd4aa614 `git clone` checkout, no env | git | 1,147,680 | `f65eff3d…6e17` (= A) |
| D fd4aa614 archive, no env | none | 1,147,680 | `8c7b380eee64949ea008e6b054587ae6cbe73ee73305a0d926b444b6233c6d18` (stamp `unknown`) |
| E fd4aa614 archive + env, separate rebuild for VPS (09:41) | env | 1,147,680 | `f65eff3d…6e17` (= A) |
| aarch64 musl fd4aa614 (rust-lld), qemu `--version` ok | git | 973,904 | — |

Binary size unchanged vs base (the stamp fits in existing padding). The base
recipe still reproduces the pinned candidate sha256, so the pair runbook's
artifacts are unaffected.

## Real test (`GALION_REAL=1`), 09:38:35-09:39:58

`GALION_REAL=1 GAIA_CORTEX_REAL_TMP=/workspace/swarm/target/cortex-version-commit-real cargo test -p gaia-cortex --test real_build_stamp -- --nocapture`
-> **1/1 ok** (82.95 s; 4 release builds 20.0-21.3 s each):

```
A 1147680 B sha256 f65eff3d… --version "gaia-cortex 0.1.0 (commit fd4aa614, built 2026-10-28T09:38:26+01:00, features llm-advisor)"
B 1147680 B sha256 f65eff3d… (same)
C 1147680 B sha256 f65eff3d… (same)
D 1147680 B sha256 8c7b380e… --version "gaia-cortex 0.1.0 (commit unknown, built unknown, features llm-advisor)"
22470/22471/22472 /health after 100 ms
A /health commit "fd4aa614df3057b63f3e09a97f5cd38eedb544cf" build {"commit":"fd4aa614…","date":"2026-10-28T09:38:26+01:00","features":["llm-advisor"]}
C /health same as A
D /health commit "unknown" build {"commit":"unknown","date":"unknown","features":["llm-advisor"]}
```

Processes killed, ports free, temp tree removed by the test.

## VPS (read-only live probe + throwaway run)

- 09:41:00 live `gaia-cortex` (PID 186171, NRestarts 0,
  `/opt/galion/rust/bin/gaia-cortex-73583ab-llm`) `/health`:
  `{"ok":true,"service":"gaia-cortex","version":"0.1.0",…,"mode":"live","nerve":true}`:
  no commit, so F-357-1 is real on live.
- 09:41:51-09:42:20 build E copied to `/var/tmp/stamp-377-20261028094141`, run as
  `nobody`: `sha256sum` = `f65eff3d…6e17`; `--version` = the line above;
  `serve` on 127.0.0.1:22470 (dry-run, no nerve, no LLM) up in ~100 ms,
  `/health` `commit` = `fd4aa614df30…44cf`,
  `build.date` = `2026-10-28T09:38:26+01:00`. Then killed; port 22470 free;
  dir removed; 0 leftover processes. The first attempt failed before start
  (`env -i` dropped PATH, so `runuser` was not found; nothing ran) and was re-run.
- Live cortex after: same PID 186171, NRestarts 0. Nerve 127.0.0.1:8670 `/state` read
  ok (open galion-one-mouth 503 incident, unrelated).

## Gates

- `cargo fmt --all -- --check` ok; `cargo clippy -p gaia-cortex --all-targets -D warnings`
  ok (default and `--features llm-advisor`).
- `cargo test -p gaia-cortex --all-targets`: **110/0** default and **110/0**
  llm-advisor (lib 45 = 42 + 3 build_info; binary_e2e 2 = 1 + new
  `version_prints_build_stamp`, `/health` commit/build asserted; real suites
  gated skip-pass).
- aarch64 musl release ok; `cargo check` aarch64-linux-android + armv7 musleabihf ok.

## Findings / owner decisions

- D-377-1: the staged pair candidate (`58a12e61`, sha `7ca6512b…`) has no stamp.
  Option A: keep the pair as staged (sha256 stays the identity, F-357-1 for that
  deploy). Option B: land this on the cortex line and rebuild the candidate
  (new sha in `pair.json`, re-run pair_preflight). Recommend A now, B for the
  next cortex deploy.
- D-377-2: deploy recipes that build from `git archive` must pass
  `GALION_BUILD_COMMIT` + `SOURCE_DATE_EPOCH`, or the binary says `unknown`.
  Recommend adding both to galion-deploy / the runbook build step.
- F-377-1: the nerve (`openmind-nerve --version`) still has no commit (other half
  of F-357-1, outside this row); the same `build.rs` pattern ports over.
- F-377-2: a dirty working tree is NOT marked (no `-dirty`), on purpose: a dirty
  check would need re-running on every file change and breaks one commit = one
  binary. Deploy builds come from clean commits or archives.
