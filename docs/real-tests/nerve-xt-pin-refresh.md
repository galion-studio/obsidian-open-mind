<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve real_cross_target: pin refresh (slot 376, F-363-2 / D-363-2)

- Branch `swarm/nerve-xt-pin-refresh`, base `swarm/real-test-port-env` 0d545118.
- Times Europe/Zagreb, CET +01:00. Slot ports 22460-22469 (used 22460-22464).
- File: `crates/openmind-nerve/tests/real_cross_target.rs` (test only, no product code).

## Problem

`real_cross_target` pinned the aa77864 binary (f0994b6d..., 1 213 216 B). That
binary is no longer live and the pin can only pass on the aa77864 tree, so the
default run failed on every later commit (F-363-2).

## Live state (read-only VPS probe, 2026-10-28 08:10:06 +01:00)

- `galion-nerve.service` active, NRestarts 0, since 2026-10-27 18:15:00 +01:00,
  ExecStart `/opt/galion/rust/bin/openmind-nerve-9d1a4f3 serve --mode event`.
- `sha256sum` d0c70b879e5866bb105707d881686757b5732168da2c016f791316ec3f521d45,
  1 458 760 B (older on disk: aa77864 f0994b6d 1 213 216 B, bb287cc c15be071 1 415 176 B).
- Commit 9d1a4f3 = tip of `swarm/nerve-deploy-t273u` (deploy t273u).
- `GET 127.0.0.1:8670/health` 200, cycle 66, score 64, system degraded; wire
  key order sorted: appended, cycle, score, service, status, system, ts, version.
- Nothing changed on the VPS; nothing deployed.

## Reproducing the live binary (box, ~08:12 +01:00)

`git archive 9d1a4f3` into a scratch dir, then
`cargo build --release --locked --target x86_64-unknown-linux-musl -p openmind-nerve`:

| linker | size | sha256 | equals live |
|---|---|---|---|
| default (committed `.cargo/config.toml`) | 1 463 072 | 64ce3aa0... | no |
| `rust-lld` (deploy worktree had it as an uncommitted config edit) | 1 458 760 | d0c70b87... | **yes, bit for bit** |

So the deploy recipe is: solo `-p openmind-nerve`, x86_64 musl, release,
`--locked`, linker rust-lld, toolchain rustc 1.99.1 (58cf8f9c3 2026-10-13).

## The two options, both staged

`NERVE_XT_PIN` picks the mode (the slot-363 `NERVE_XT_LIVE_PIN` still works
when `NERVE_XT_PIN` is unset: `0` = budget, `1` = aa77864).

- **B. `budget` (new default): pin retired.** No SHA pin. Every binary must
  fit its row of the size-budget table (`SIZE_BUDGET`: 2 MiB hard, warn at
  90 % = 1 887 435 B), and the solo and live-recipe x86_64 builds (separate
  target dirs, different package sets) must be bit-identical. Never goes
  stale on a redeploy; still catches size creep and feature-unification drift.
- **A. `live`: re-pin to the live build, reproducibly.** Everything in B,
  plus: `git archive` of the live commit (`NERVE_XT_LIVE_REV`, default
  9d1a4f3) is rebuilt with the deploy recipe and must equal the VPS binary
  (`NERVE_XT_LIVE_SHA`/`_SIZE`, default d0c70b87... / 1 458 760). Because it
  builds the pinned commit from its own tree, it does not rot on every
  commit (the aa77864 pin did); it only goes stale when the nerve is
  redeployed or the toolchain changes, and then the env knobs bump it without
  a code change. The rebuilt live binary is also smoked on the slot ports and
  its `/health` key set must match the tree's.
- `aa77864`: the old slot-217 pin, kept for history (passes only on that tree).

Always-on unit tests (no GALION_REAL): `pin_mode_parsing`,
`size_budget_table`, `live_pin_verdict`, `sha256_known_answers`: 5/5 pass
(incl. the gated test as skip).

## Real runs (GALION_REAL=1, NERVE_XT_PORT_BASE=22460)

| time (+01:00) | mode | result | key lines |
|---|---|---|---|
| 08:14:46-08:16:28 | default (budget) | 1/1 pass, 102 s | x86_64 1 930 048 B ccc551fb (WARN: above 90 %, headroom 167 104 B); aarch64 1 591 776 B 6a245cb8; recipe == solo ccc551fb; +471 288 B vs live 9d1a4f3; smokes :22460/:22461 (qemu)/:22462 first cycle 0.10/0.21/0.10 s, /health wire order equal on all 3; android + armv7 check ok |
| 08:16:35-08:16:57 | `live` | 1/1 pass, 22 s | `live pin OK: rebuild of 9d1a4f3 == VPS binary d0c70b87... 1458760 B` (build 20.4 s); live-pin smoke on :22462 first cycle 0.10 s, `/health` wire order sorted (same as the real VPS answer above), /state 23 keys vs tree 24 (tree adds `edge_hosts`) |
| 08:17:07 | `aa77864` | fails as intended | `live recipe no longer reproduces the VPS binary` |
| 08:17:08 | legacy `NERVE_XT_LIVE_PIN=1` | fails as intended | same as aa77864 |
| 08:17:08 | `live` + `NERVE_XT_LIVE_SHA`=aa77864 sha | fails as intended | `rebuild of 9d1a4f3 = d0c70b87... 1458760 B, pinned VPS binary = f0994b6d... : either the live nerve was redeployed (bump ...) or the toolchain/recipe changed` |
| 08:17:09 | `live` + `NERVE_XT_LIVE_REV=deadbeef` | fails as intended | `commit deadbeef not in this repo: git fetch <remote> swarm/nerve-deploy-t273u` |
| 08:17:09 | `NERVE_XT_PIN=yes` | fails as intended | `NERVE_XT_PIN must be budget, live or aa77864` |
| 08:17:09 | legacy `NERVE_XT_LIVE_PIN=0` | 1/1 pass | maps to budget |
| 08:28:55-08:29:16 | `live`, cold (pin target dir and scratch builds deleted) | 1/1 pass, 21.6 s | live-pin rebuilt from scratch in 20.3 s, d0c70b87... 1 458 760 B == VPS |

Ports 22460-22464 free after every run (the test asserts each nerve port is
closed after its kill).

## Findings

- **F-376-1** The tree nerve (0d545118 lineage) is 1 930 048 B: +471 288 B
  (+32 %) over live 9d1a4f3 and 92 % of the 2 MiB ceiling. `cargo tree`
  (x86_64 musl, normal deps) adds galion-common, tunnel-lint, semver,
  indexmap/hashbrown/foldhash/equivalent (serde_json preserve_order). The
  budget mode prints a WARN for it today; the next ~167 KB of growth fails.
- **F-376-2** Wire order of `/health` changes at the next nerve deploy:
  live 9d1a4f3 sends keys sorted (no preserve_order), the tree sends
  insertion order (status, service, version, ...). JSON consumers do not care;
  anything matching raw text would.
- **F-376-3** `/state` gains `edge_hosts` in the tree (24 keys vs 23 live).
- **F-376-4** The live recipe depends on an uncommitted linker line
  (`[target.x86_64-unknown-linux-musl] linker = "rust-lld"` in the deploy
  worktree's `.cargo/config.toml`). With the committed config the same commit
  gives a different binary (1 463 072 B). Commit the line (or pass the env as
  the test does) so deploys are reproducible from git alone.

## Recommendation (owner decision D-363-2 -> D-376-1)

Recommend **B (`budget`) as the default** and keep **A (`live`) as an opt-in
deploy gate**: run `NERVE_XT_PIN=live` right after each nerve deploy (bump the
three env knobs or the constants to the new rev/sha/size). B never goes stale
on its own, so a red default run means a real problem (size or unification);
A proves the live binary is exactly what git says, which is the property the
old pin wanted, without breaking every later commit. This branch already
makes B the default; if the owner prefers A as default, flip
`pin_mode_from(None, None)` to `Live`.

- D-376-2: commit the x86_64 rust-lld linker line (F-376-4)? Not done here
  (changes every x86_64 musl build in the workspace).

## VPS after the runs (read-only, 2026-10-28 08:29:23 +01:00)

Live binary sha still d0c70b87..., `galion-nerve` active, NRestarts 0;
nerve `/state` 200 (alerts.external "none configured"); cortex
`127.0.0.1:8674/health` 200; no VPS listener on 22460-22469.

## Gates (box, 2026-10-28 08:29-08:43 +01:00)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets
  --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --no-fail-fast` (excl. openmind-sandbox,
  cgroup-detect, hermes-tools; sandbox_vps_only): 378 suites, 2011 passed,
  0 failed, 18 ignored. (A first run at ~08:27 hit the known room
  `default_and_fixture_do_not_open_a_socket` fd flake, fixed on
  swarm/room-fd-flake facc119e, not in this base; it passed on the re-run.)
- `scripts/no-python.sh` ok. No new deps; Cargo.lock unchanged.
