<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 393 nerve-version-commit: real-test record

Branch `swarm/nerve-version-commit` off nerve-merge-6 `9baa450c` (the staged
nerve of the cortex+nerve pair). Fixes F-377-1: the nerve half of F-357-1
(`openmind-nerve --version` and `/health` showed no commit). Ported from the
cortex stamp (swarm/cortex-version-commit `b36dc358`). All times
Europe/Zagreb CET (+01:00), 2026-10-28. Box loopback ports 22630-22632; VPS
throwaway `/var/tmp/stamp-393-20261028154405` as `nobody` on 127.0.0.1:22630.
**No deploy**: no live file, unit, permission or service changed. **No new
dependencies** (std `build.rs` + existing `serde_json` / `zagreb-time`).

## What changed

- `crates/openmind-nerve/build.rs` (std only, same logic as the cortex one):
  - commit: `GALION_BUILD_COMMIT` (7-64 hex, else the build fails) if set, else
    `git rev-parse HEAD` **only if** the git top level is this workspace, else
    `unknown`;
  - date: `SOURCE_DATE_EPOCH` if set, else the commit's committer date, else
    `unknown`. **Never the wall clock**: one commit = one binary.
  - Re-stamps when HEAD / the branch ref / packed-refs change.
- `src/build_info.rs`: `COMMIT`, `short_commit()`, `date()` (Zagreb RFC 3339,
  never `Z`), `features()` (`journal-native` when compiled in), `version_line()`,
  `json()`.
- `openmind-nerve --version` / `-V`:
  `openmind-nerve 0.1.0 (commit fc5c1c9c, built 2026-10-28T15:42:07+01:00, features none)`
- Startup log line gains a trailing ` commit=<short>` (after `components=N`;
  the `listening on ADDR` part that tests and readiness checks match is unchanged).
- `/health` gains `"commit": "<full sha>"` and
  `"build": {"commit", "date", "features"}`; all old keys stay
  (`status, service, version, appended, ts, system, score, cycle`).
  `/state` is unchanged (no golden churn).

### Build recipe (archive builds)

```
GALION_BUILD_COMMIT=<sha> SOURCE_DATE_EPOCH=$(git show -s --format=%ct <sha>) \
  cargo build --release --locked -p openmind-nerve --target x86_64-unknown-linux-musl
```

Without the two variables a `git archive` build says `commit unknown, built unknown`.

## Real test (`GALION_REAL=1`), 15:39:56-15:41:39

`GALION_REAL=1 NERVE_STAMP_REAL_TMP=/workspace/swarm/target/nerve-version-commit-real cargo-slot test -j 2 -p openmind-nerve --test real_build_stamp -- --nocapture`
-> **1/1 ok** (102.99 s; 5 release builds of ~20 s each). HEAD was the code
commit `9038c504`:

| build | source | bytes | sha256 |
|---|---|---|---|
| A 9038c504 `git archive` dir 1 + env stamp | env | 1,483,552 | `f565dea7754288a041dd6a28f6a3c71d3a32532c795947fe4c8daeef03940c70` |
| B same, other dir/depth, other target dir + env | env | 1,483,552 | `f565dea7…0c70` (= A) |
| C `git clone` checkout, no env | git | 1,483,552 | `f565dea7…0c70` (= A) |
| D `git archive`, no env | none | 1,483,552 | `f0b512e61aaccaece712a24c9d1b77c4bb4eb13b912b29339c39378c51512bf0` (stamp `unknown`) |
| Z base 9baa450c `git archive` (no stamp code) | — | 1,479,456 | `8c9d7a5fa4f84d6d56d6185dfd5b71171c8ba5b17bf49c0bde7fb5503f6593da` |

```
A startup "<6>openmind-nerve 0.1.0 listening on 127.0.0.1:22630 interval=3600s mode=dry-run data=… event-log=off components=3 commit=9038c504"
A /health commit "9038c50456ee928806786652aff052497d5c8654" build {"commit":"9038c504…","date":"2026-10-28T15:39:42+01:00","features":[]}
C startup/health: same as A (port 22631)
D startup "… commit=unknown"; /health commit "unknown" build {"commit":"unknown","date":"unknown","features":[]}
```

Processes killed, ports free, temp tree removed by the test. The test fails if
the stamp costs more than `NERVE_STAMP_MAX_DELTA` (default 4096 B) vs the base.

### Size delta (F-376-1)

- x86_64 musl, default features: **+4,096 B** (one page: 1,479,456 -> 1,483,552)
  vs the unstamped nerve-merge-6 base. Stamped vs unknown-stamp: same size.
- Headroom on this line (2 MiB hard budget, 2,097,152 B): 613,600 B. On the
  nerve-xt-pin-refresh line (1,930,048 B, headroom 167,104 B) the same page
  would leave ~163,008 B; that line is already at 92% WARN, and the stamp does
  not change the verdict.
- aarch64 musl (rust-lld) release fc5c1c9c: 1,222,400 B; qemu `--version` =
  `openmind-nerve 0.1.0 (commit fc5c1c9c, built 2026-10-28T15:42:07+01:00, features none)`.
- `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf: ok.

## VPS (read-only live probe + throwaway run)

- 15:43:15 live `galion-nerve` (MainPID 184014, NRestarts 0,
  `/opt/galion/rust/bin/openmind-nerve-9d1a4f3 serve --mode event`) `/health`:
  `{"appended":180,"cycle":97,"score":64,"service":"openmind-nerve","status":"ok","system":"degraded",…,"version":"0.1.0"}`:
  no commit, so F-377-1 is real on live. Live `--version` (15:44:15) =
  `openmind-nerve 0.1.0`. `/state` 200, cortex 8674 `/health` 200.
- 15:44:15-15:44:16 archive+env build of `fc5c1c9c` (1,483,552 B, sha256
  `68be464e73cd9a1f20487ac709bee59d368f0da36b12b3798f91a47673f6beea`) copied to
  the throwaway dir and run as `nobody` (`env -i`, dry-run, systemd/journal probes
  off, one dead component): `--version` as above; `serve` on 127.0.0.1:22630
  `/health` = `{"appended":0,"build":{"commit":"fc5c1c9c1c1a374d0e691a4ca8dff4b8c027b005","date":"2026-10-28T15:42:07+01:00","features":[]},"commit":"fc5c1c9c…b005",…,"service":"openmind-nerve","status":"ok",…}`;
  startup line ends `commit=fc5c1c9c`. Then killed: 0 leftover processes,
  port 22630 free, dir removed. Live nerve after: same MainPID 184014,
  NRestarts 0, `/state` 200.

## Gates (box)

- `cargo fmt --all -- --check` ok.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok;
  `-p openmind-nerve --features journal-native` ok.
- `env -u GALION_REAL cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect`
  (F-383-10, sandbox_vps_only): **1363 passed / 0 failed / 6 ignored**, 288 result lines, rc 0.
- `cargo test -p openmind-nerve`: lib 122/0 (incl. 3 new build_info + /health
  assert in `server` test), new `version_stamp` 2/0 (`--version`/`-V`, startup
  line + `/health` on an ephemeral port), all other suites green;
  `--features journal-native` 189/0.
- `scripts/no-python.sh` ok; SPDX headers on all new files.

## Findings / owner decisions

- D-393-1 (same shape as D-377-1): the staged pair nerve (`9baa450c`) has no
  stamp. A: keep the pair as staged (sha256 stays the identity). B: land this on
  the nerve line, rebuild the candidate (new sha in `pair.json`, re-run
  pair_preflight). Recommend A for the current pair, B for the next nerve deploy.
- D-393-2 (= D-377-2): deploy recipes building from `git archive` must pass
  `GALION_BUILD_COMMIT` + `SOURCE_DATE_EPOCH`, or the nerve says `unknown`.
- F-393-1: `/health` gains two keys; consumers (gaia-live `healthy`, galion-deploy
  health table, galion-realtest probes) only read `status`/route codes, checked by
  `consumer_compat` 11/11.
- F-393-2 (= F-377-2): a dirty tree is not marked `-dirty`, on purpose.
- The live nerve is `9d1a4f3` built with the x86_64 rust-lld line (F-376-4); the
  stamp is independent of the linker.
