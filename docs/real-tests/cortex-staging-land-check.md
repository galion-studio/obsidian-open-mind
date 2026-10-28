<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cortex-staging-land-check: D-293-4 land gate (wave-4 slot 317)

Date: 2026-10-27, 22:24-22:52 CET (+01:00), Europe/Zagreb. Box only; the VPS was only read.
Branch `swarm/cortex-staging-land-check` is a throwaway gate branch. **main-staging and main were not
updated. Nothing was deployed.** Landing it is still owner decision D-293-4.

## What was merged

| | |
|---|---|
| first parent | `3f015986` = tree snapshot of GitHub `swarm/main-staging` **38a07700** (PR #13 merge) on top of 98466ab9. Tree `4864589f` is byte-identical to 38a07700's tree (checked again at 22:25 CET via the GitHub API: main-staging head still 38a07700, tree 4864589f). 38a07700 objects are not on the box, so the snapshot (made by slot 312) stands in for it. |
| second parent | `swarm/cortex-staging-fold` **77df80f9** (slot 293: main-staging 98466ab9 + cortex-live-merge-3 73583abf) |
| merge | `git merge --no-ff 77df80f9`: **0 conflicts**. Auto-merged only `.github/workflows/self-eval.yml` (PR #13 userns step in `test` + fold's new `replay-corpus` job) and `Cargo.lock`; no conflict markers. 101 files, +21311/-23 vs 38a07700. |

Because the first parent is a tree copy, a real land onto 38a07700 would produce a merge commit with a
different SHA but the same tree (when nothing else lands first).

## Gates (own target dir, `cargo-slot -j 2`)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok (only vendored candle `f64::INFINITY` deprecation warnings, not a workspace member) |
| `cargo clippy -p gaia-cortex --all-targets --features llm-advisor -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --no-fail-fast` (22:36-22:40) | **399 suites, 2135 passed / 0 failed / 18 ignored** |
| `cargo test -p gaia-cortex --features llm-advisor` | 85 passed / 0 failed |
| `scripts/no-python.sh` | ok |
| `repo-check --root .` | PASS, 2204 files, 0 errors, 83 warnings |
| `cargo xtask replay-corpus` | ok: 11 replayed, 0 skipped; 4/4 tests |

`hermes-tools` was left out on purpose: its `sandboxed` test runs the sandbox, and sandbox runs are VPS-only
(sandbox_vps_only). The fold changes no file in hermes-tools, openmind-sandbox or cgroup-detect.

## Real tests (release, box loopback, ports 21870-21879)

The merge-3 real tests hard-code slot 258's ports 21270-21279 and t42's 20620-20629. For this run they were
moved to 21870-21879 in the working tree only and reverted afterwards (not committed). `real_t51_chaos`
used `GALION_T51_PORT_BASE=21870`. Nerve = this merge's release `openmind-nerve` (1 894 232 B, sha256
c582571c83acf668...), cortex = release `gaia-cortex --features llm-advisor` (1 028 696 B, sha256
0daa05a234ea2eb1...); same sizes as the slot 293 fold. Logs: `/workspace/swarm/logs/317/` (box).

| Test (CET) | Result |
|---|---|
| `real_nerve_io_unlock` 22:42:26-22:43:01 | 1/1. H1 decision 3468 ms, /stats 200 in 0 ms during the hang, ack 404 in 0 ms; H2 3316 ms; H3 3 batches, last 6649 ms |
| `real_api_load` 22:43:01-22:43:23 | 1/1. p99 /health,/stats: quiet 0.57/0.63, flood 1.12/1.24, hung decider 0.97/0.83 (busy 117), hung+flood 0.85/0.86 ms; 0 failed |
| `real_t51_chaos` 22:43:23-22:44:40 | 2/2. S6 1 s latency decided 4531 ms; S7 hung nerve 3638 ms, /health+/stats 0 ms; S8 re-subscribed 906 ms; S9 llm_calls 0, non-loopback sockets 0; OpenRouter/Hermes llm endpoints refused (exit 2) |
| `real_t237_backfill` 22:44:40-22:45:11 | 2/2. P1 backfill 3/3 in 505 ms; P2 cleared 506 ms; P3 cortex resolve +1032 ms; P4 reconciled 1579 ms, remapped (live shadow sub-test skipped: no `GALION_T237_LIVE_PORT`) |
| `real_flap_dedupe` 22:45:11-22:46:10 | 1/1. R2 after 4 folded writes / 2489 ms; caps R3-cap, R3-cap-total at 6/h |
| `real_cortex_t42` 22:49:24-22:49:45 | 1/1. Replay 6 plans in 7 ms (5 families); live: 800 flood requests, all 5 families' plans after 5.5 s; stray + voice plans gone 4.5 s after the fix |
| `real_replay_corpus` + `GALION_REAL_VPS=1` 22:47:43-22:49:10 | 3/3. Binary replays 11/11 captures, all match, 0 flaps; fresh read-only VPS capture 64 D, 0 flaps, incidents {edge, galion-one}; live nerve cycle 26 = 64 D, same incidents, no live-only incidents |

The first `real_cortex_t42` attempt (22:46) failed only because the port remap turned the nerve's
`NERVE_LISTEN_SCOPE` "20620-20629" into an invalid "21870-20629" range (a remap error of this run, not a code
fault). After fixing the range in the working tree it passed. No listeners left on 21870-21879.

Note: `real_t51_chaos::llm_advisor_cannot_be_enabled` still hard-codes `127.0.0.1:20974` (slot 227's range);
the cortex refuses to start there, so it does not bind, but the constant should follow `GALION_T51_PORT_BASE`.

## Live VPS, read-only (22:50 CET)

- gaia-cortex active, `/opt/galion/rust/bin/gaia-cortex-73583ab-llm` (= the cortex in this merge),
  `127.0.0.1:8674/health` ok, mode live, llm on, hermes_handoff on, incidents_open 2, escalations_open 1.
- galion-nerve active, `/opt/galion/rust/bin/openmind-nerve-9d1a4f3` (nerve-deploy-t273u, not in this merge);
  `/state` 200, cycle 26, score 64 D.
- No writes, no restarts, no deploy.

## Owner decisions

- **D-293-4 (ready)**: the land of cortex-staging-fold 77df80f9 onto main-staging 38a07700 is clean and every
  gate above is green. Landing (updating main-staging) is a separate explicit step for the owner / main-merge run.
- D-293-2 (ownership rows) and D-293-3 (corpus re-bless 66 D -> 70 C, `NERVE_EDGE_HOSTS=0` pin for
  live-replay207) are carried in unchanged from the fold.
- Follow-up (not an owner decision): make the real-test ports configurable by env like t51, so gate runs do not
  need working-tree remaps.
