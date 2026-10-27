<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# cortex-staging-fold: merge check of cortex-live-merge-3 into main-staging (wave-4 slot 293)

2026-10-27 19:14-19:45 CET (+01:00). Throwaway branch `swarm/cortex-staging-fold`, base main-staging
98466ab9, merged `swarm/cortex-live-merge-3` 73583abf (the cortex source that is live on the VPS as
`gaia-cortex-73583ab-llm`). **main-staging and main are NOT updated. Nothing deployed.**

## Result in one line

The fold is mechanically doable (8 conflicted files resolved, 3 follow-up fixes) and everything is green
**except 6 tests in 2 nerve suites (+1 real test): the T4.2 HADES plan templates from merge-3 are refused
by main-staging's hardened hades-gate (S3.1 allow-list, e6ceca7)**. That is an owner decision (D-293-1),
not a merge error, so main-staging should not take this fold until it is settled.

## What merge-3 brings that main-staging lacks

`crates/gaia-cortex` (absent on staging), `crates/nerve-cortex-contract`, `crates/xtask` (`cargo xtask`
CI runner, replay-corpus), nerve T4.2 plans (`plans.rs`, `protected_hosts.rs`,
`deploy/nerve/plans-protected-hosts.conf`), T5.3 replay corpus (`corpus.rs`, 8 captures + expected
verdicts), galion-common `Limits::evict_idle` (cortex API slowloris guard), deploy/gaia-cortex Hermes
handoff units. 43 commits; the fold diff vs staging is 95 files, +20 567 / -23 lines.

## Conflicts and how they were resolved (commit ca306c3, a merge with 2 parents)

| File | Resolution |
|---|---|
| `.cargo/config.toml` | union: staging rust-lld linkers for aarch64/armv7 musl + merge-3 `[alias] xtask` |
| `Cargo.lock` | staging lock + `cargo metadata --offline` (adds nerve-cortex-contract, xtask, gaia-cortex deps) |
| `openmind-nerve/src/lib.rs` | module union (`owners`, `egress`, `evt`, `reflex_audit` + `corpus`, `plans`, `protected_hosts`) |
| `openmind-nerve/src/config.rs` | union: `port_owners` / `unowned_ports` + `plans_protected` |
| `openmind-nerve/src/engine.rs` | `with_specs` -> `with_specs_env(cfg, specs_list, env)`; state JSON gets both `load_note` (staging) and `cortex_plans` (merge-3) |
| `openmind-nerve/src/main.rs` | docs + USAGE union: `replay --reflex/--stretch/--worst-case`, `reflex-audit` + `replay --plans/--verdict/--hold`, `plans STATE.json` |
| `openmind-nerve/src/replay.rs` | real semantic merge: one `replay_dir_full(dir, cfg, env, stretch, hold, setup, on_cycle)`; `replay_dir_with(dir, cfg, stretch, setup)` keeps staging's T3.2 stretch API (reflex_audit), new `replay_dir_hold(dir, cfg, env, hold, on_cycle)` is merge-3's corpus API (corpus.rs renamed its call). Plan = capture cycles, then stretch cycles, then quiet hold cycles of the last planned snapshot; `on_cycle` after every cycle; ss port_procs + missing-sample handling from staging kept |
| `galion-common/src/http.rs` | `Limits { accept_chunked, evict_idle }` both; staging's `serve_connections(_with)` + `ConnHandler` API kept; merge-3 evict loop moved into a private `serve_hooked`; only `serve()` (built-in request reader) reports "request read" so a connection leaves the evictable set; a plain `ConnHandler` stays evictable until it returns (evict_idle is off by default, the cortex API uses `serve()`). `handle_connection_with` -> private `handle_connection_hooked` with the read-done hook |

Follow-up commits on the branch:

- (inside the merge commit) `hermes-rs` / `openmind-memory` `Limits {..}` literals get `evict_idle: false` (compile fix, behaviour unchanged).
- `gaia_core::ownership::CRATES`: gaia-cortex + nerve-cortex-contract -> Nerve, xtask -> CargoBuild, all "pending Slavik" (D-293-2); `gaia` `ownership_counts` 68/56 -> 71/59.
- Replay corpus re-blessed for the folded nerve; 3 new captures listed (fixture:consumer from staging, live:replay200, live:replay207 on the box). See "Corpus drift".
- `plans.rs` unit test IP 51.15.0.1 -> TEST-NET-2 198.51.100.1 (staging repo-check `hosts` rule: 1 error -> 0).

## Gates (box, `cargo-slot`, -j 2, own target dir)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok (only the pre-existing vendored candle `f64::INFINITY` deprecation warnings, not workspace members) |
| `cargo clippy -p gaia-cortex --all-targets --features llm-advisor -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --no-fail-fast` | 406 suites, **2150 passed / 6 failed / 18 ignored**; failing: `openmind-nerve --test cortex_protected_hosts` 3/6, `--test cortex_t42_plans` 3/5 (D-293-1) |
| first run, before the follow-ups | additionally failed gaia-core `workspace_ownership`, gaia `ownership_counts`, nerve `replay_corpus`, repo-check 1 error (all fixed by the follow-up commits) |
| `cargo test -p gaia-cortex` default / `--features llm-advisor` | default 78 passed + 7 real tests skipped without GALION_REAL; llm-advisor 85/0 |
| `scripts/no-python.sh` | ok |
| `repo-check --root .` | PASS, 2200 files, 0 errors, 83 warnings (was 1 error before the TEST-NET fix) |

Sandbox crates were not run locally (sandbox_vps_only); this fold does not touch them.

## D-293-1: T4.2 plans vs the hardened hades-gate (blocking)

merge-3's T4.2 plan templates were written against the older hades-gate. Staging carries hades-redteam
S3.1 (e6ceca7, "48 -> 0 bypasses") and the Option B typed-steps prototype (0f41f29). The S3.1 rules refuse:

- `ingress-rule-removal`: `AdminAccount("network change")` - the words `route(s)` / `ip` are in the hazard list (`safety.rs:218-219`).
- `edge-rate-limit`: `Opaque("source")` ("single source of the flood"; `safety.rs:233`) and "statement does not start with an allow-listed plan verb" (`safety.rs:382`).

Failing tests: `cortex_protected_hosts` (3), `cortex_t42_plans` (`every_family_yields_a_well_formed_plan`,
`a_template_goes_through_a_real_single_use_approval`, `golden_replay_yields_its_plans_through_the_engine`),
real `real_cortex_t42_on_the_box` (6 plans built in 8 ms, then `plan-edge-rate-limit-cloudflare-edge-tuner`
refused). HADES was **not** touched (no loosening). Options for Slavik:

- A. Reword the T4.2 templates into the S3.1 vocabulary (allow-listed verbs, no `route`/`ip`/`source` words), re-gold, keep the gate as is. Smallest; cortex-side only.
- B. Emit the plans as Option B typed signed steps (0f41f29) instead of prose. Matches the HADES A/B/C decision that is still open.
- C. Keep T4.2 out of main-staging (fold gaia-cortex only, drop nerve plans.rs) until A or B lands.

Recommendation: A now (it unblocks the fold), B later if Slavik picks HADES option B.

## Corpus drift (re-blessed, D-293-3)

The folded nerve (staging coverage checks + correlation incidents in `findings`) changes the T5.3 expected
verdicts, **0 flaps on every capture**:

- fixture-cortex-replay 19 changes (new `info coverage ...` checks, `incidents:*` findings); fixture-cortex-sshd 72; live-replay1 15.
- live-replay158/179/186/192/193: 26 changes each, **score 66 D -> 70 C** (final and held).
- New: fixture-consumer, live-replay200, live-replay207. live-replay207 (one snapshot, 5 min of journal after it) flapped `grade:C 111111111100011111111`: the T2.12 edge-host-rate warn (`workspace*.app`) opens in hold cycles 10-12 from the post-snapshot journal burst and clears, grade C -> D -> C. Pinned `NERVE_EDGE_HOSTS=0` for that capture (the corpus' per-capture sensor env), noted in corpus.json. Owner: keep the pin, or teach `corpus::flaps` that a grade dip caused by one incident onset+clear is one episode.

Full drift list: `/workspace/swarm/logs/cortex-staging-fold-corpus-drift.txt` (box only).

## Real tests (release, box loopback, ports 21630-21639)

The merge-3 real suites hard-code slot 258's 21270-21279 (and t42 20620-20624); for this run they were
temporarily pointed at 21630-21639 (not committed). Nerve binary = **this fold's** release
`openmind-nerve` (1 893 976 B), cortex = this fold's release `gaia-cortex --features llm-advisor`
(1 028 696 B). So the folded nerve and the cortex were exercised together. Logs:
`/workspace/swarm/logs/cortex-staging-fold-real*.log`.

| Test (CET) | Result |
|---|---|
| `real_nerve_io_unlock` 19:34:37-19:35:17 | 1/1. H1 decision 3304 ms, /stats 200 in 0 ms during the hang, ack 404 in 0 ms; H2 3323 ms; H3 3 batches, last 6663 ms |
| `real_api_load` 19:35:17-19:35:40 | 1/1. p99 /health,/stats: quiet 0.46/0.55, flood 0.92/0.81, hung decider 0.91/0.83 (busy 114), hung+flood 0.91/0.80 ms; 0 failed |
| `real_t51_chaos` (`GALION_T51_PORT_BASE=21630`) 19:35:40-19:36:58 | 2/2. S1 503 ms; S7 hung nerve decision 3842 ms, /health+/stats 0 ms; S8 re-subscribed 905 ms; S9 llm_calls 0, non-loopback sockets 0 |
| `real_t237_backfill` 19:36:58-19:37:31 | 2/2. P1 backfill 3/3 in 504 ms; P2 cleared 507 ms; P3 cortex resolve +1017 ms; P4 reconciled 1519 ms, remapped |
| `real_flap_dedupe` 19:37:31-19:38:30 | 1/1. AFTER: R2 after 4 folded writes / 2482 ms; caps dry-run R3-cap, total R3-cap-total at 6/h; live same |
| `real_replay_corpus` + `GALION_REAL_VPS=1` 19:38:41-19:41:29 | 3/3. Binary replays 11/11 corpus captures, all match; flapping control exits 3; fresh read-only VPS capture: replay 64 D, 0 flaps, incidents {edge, galion-one}; live nerve cycle 11 = 39 F with {console, edge, gaia-cortex, galion-one, hermes-rs} (live-only: console, gaia-cortex, hermes-rs - not reproducible from /state) |
| `real_cortex_t42` 19:41:29-19:41:36 | **0/1** (D-293-1, hades-gate refuses the edge-rate-limit plan) |
| `galion-common --test http_evict` (in the workspace run) | ok |

No processes or listeners left on 21630-21639 afterwards.

## Live VPS, read-only (19:41:56 CET)

- gaia-cortex: `/opt/galion/rust/bin/gaia-cortex-73583ab-llm` (= the merged-in source), `/health` ok, mode live, llm on, hermes_handoff on, incidents_open 6.
- galion-nerve: `/opt/galion/rust/bin/openmind-nerve-9d1a4f3` (nerve-deploy-t273u; **not** in staging or in this fold - slot 292 nerve-merge-4 territory). `/state` 200.
- No writes, no restarts, no deploy.

## Owner decisions

- **D-293-1** (blocking): T4.2 plan wording vs S3.1 hades-gate: A reword / B typed steps / C leave T4.2 out. Recommend A.
- **D-293-2**: ownership rows (pending Slavik): gaia-cortex + nerve-cortex-contract -> Nerve, xtask -> CargoBuild, or other owners Slavik prefers.
- **D-293-3**: accept the corpus re-bless (66 D -> 70 C on the live captures) and the `NERVE_EDGE_HOSTS=0` pin for live-replay207, or change the flap rule.
- **D-293-4**: once D-293-1 is settled, land this fold into main-staging (a separate, explicit step; this slot did not).
