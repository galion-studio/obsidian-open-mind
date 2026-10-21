<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# integration-2: Gaia merge candidate (plan gaia.md S1.3 + S1.4 a,b,d)

Built 2026-10-20 22:45-23:05 CEST (+02:00) by swarm slot 37 (`swarm/integration-2`).
A candidate for Slavik to review. Agents do not merge main; Slavik merges
integration-1, then this branch.

Base: `swarm/integration-1` 8fd26f6. Four normal `--no-ff` merges in the
planned order, then two small fixup commits. First-parent history:

| # | Merge commit | Branch (head) | Result | Conflicts | Workspace tests after (passed / failed / ignored, suites) |
|---|---|---|---|---|---|
| 0 | 8fd26f6 | integration-1 (base) | - | - | (not re-run; first merge below adds 2 commits) |
| 1 | 1a517ca | swarm/gaia-hades-tokens (f3f41a2) | clean | none (Cargo.lock auto-merged) | 953 / 0 / 4, 206 suites |
| 2 | 4bc4ff3 | swarm/gaia-poseidon (498e3d2) | clean | none (Cargo.lock, crates/gaia/src/main.rs auto-merged) | 966 / 0 / 4, 207 suites |
| 3 | f91686d | swarm/gaia-land (9d3ec18) | conflict | `crates/gaia/src/lib.rs`, `crates/gaia/src/main.rs`, `Cargo.lock` | 984 / **1** / 4, 210 suites: `gaia-core --test workspace_ownership` fails on 7 unmapped crates (intended, fixed in F1) |
| 4 | 328a386 | swarm/lan-routing (e8169c9) | clean | none (openmind-discovery admit.rs, cmd.rs auto-merged) | 997 / **1** / 4, 213 suites (same guard failure) |
| F1 | bfaac35 | ownership rows | - | - | - |
| F2 | 862beb5 | repo-check allow marker | - | - | **999 / 0 / 4, 213 suites** (final head) |

Counts are the sum of every `test result:` line of
`cargo test --workspace --locked --no-fail-fast -j 2` (unit + integration + doc
tests). The 4 ignored are the pre-existing `#[ignore]` real tests.
Plan target "workspace tests >= 950, 0 failures": met (999 / 0).

## Conflict resolutions (merge 3, gaia-land)

Both sides added commands to the `gaia` CLI; the command sets are disjoint, so
every subcommand is kept.

- `crates/gaia/src/main.rs`
  - Module doc: one combined paragraph naming every exception to "no network,
    no processes, no file writes": `status --live` (read-only GETs), `poseidon`
    / `dispatch --queue` (loopback openmind-queue, budget read-only), `heal-tick`
    (approval-gated restart of box TEST services), and `view` (reads only the
    capture files it is given).
  - Usage: HEAD's `USAGE_HEAD` + `{POSEIDON}` placeholder kept (`usage()` splices
    `gaia::poseidon::USAGE`); gaia-land's `plan`, `dry-run`, `ownership`, `view`
    lines inserted before `--compact`, the POSEIDON block stays last.
  - The `match` arms auto-merged: `status --live`, `heal-tick`, `heal-plan`,
    `status`, `tick`, `simulate-faro`, `systems`, `poseidon`, `dispatch --queue`,
    `dispatch`, `plan`, `dry-run`, `ownership`, `view`, help.
- `crates/gaia/src/lib.rs`: kept HEAD's `status_live` (M1) and gaia-land's
  `plan`, `dry_run`, `crates_json`, `ownership_json` side by side; moved
  gaia-land's `pub mod view;` up next to `pub mod live_heal; pub mod poseidon;`.
- `Cargo.lock`: took HEAD's. gaia-land changes no `Cargo.toml`; its side only
  lacked integration-1's packages (`gaia-live`, `hades-gate`, `serde_json` deps of
  `gaia`, and `galion-boot` at another position). `cargo metadata --locked
  --offline` accepts it; all later gates run with `--locked`.

## Fixups

- **F1 (bfaac35) ownership registry.** After merge 3 the gaia-land guard
  `every_workspace_crate_is_mapped` failed, as it should, listing exactly
  `gaia-live, galion-common, galion-realtest, galion-voice-cpu, hermes-tools,
  nerve-client, openmind-queue` (the S2.2 list). One row each in
  `gaia_core::ownership::CRATES` (53 crates, 41 system crates). Judgement calls
  have a `why` starting with `pending Slavik`, and `gaia ownership` now prints
  `"pending_slavik": true|false` per crate (`CrateOwner::pending_slavik`).
  Tests updated: `core_owns_no_system` (gaia-live core), system-crate count
  35 -> 41, `ownership_counts` and `real_cli` 46 -> 53, new
  `pending_rows_are_flagged`. `docs/gaia-ownership.md` crate table regenerated
  from `gaia ownership` output (the 46 old rows are byte-identical) plus an
  "integration-2 rows" section with alternatives.
  Negative check: deleting the `nerve-client` row makes the guard fail with
  `crates not mapped ...: ["nerve-client"]`; restored, 3/3 pass.
- **F2 (862beb5)**: repo-check `hosts` error on lan-routing's new test line
  `crates/openmind-discovery/src/routing.rs:538` (`http://203.0.113.8:80` since slot 225, earlier a public DNS literal; a reject
  case, never dialled). Added the same `// repo-check: allow (test fixture
  address, never dialled)` marker integration-1 used for admit/cmd/probe.

| Crate | System / function | Pending Slavik? | Alternatives |
|---|---|---|---|
| gaia-live | core (no system) | yes | guardian-watchdog / MINERVA; healer / DEMETER |
| galion-common | zagreb-clock / AETHER | yes | core |
| galion-realtest | repo-check / MINERVA | yes | cargo-build / HEPHAESTUS |
| galion-voice-cpu | speech-io / AETHER | no | (as openmind-speech) |
| hermes-tools | terminal / HEPHAESTUS | yes | hermes-agents / ARTEMIS |
| nerve-client | nerve / AETHER | no | (as openmind-nerve) |
| openmind-queue | queues / POSEIDON | yes (S2 A/B) | HEPHAESTUS under option B |

## Gates on the final head (862beb5)

| Gate | Command | Result |
|---|---|---|
| fmt | `cargo fmt --all --check` | ok |
| clippy | `cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings` | ok, 0 warnings |
| tests | `cargo test --workspace --locked --no-fail-fast -j 2` | 999 passed, 0 failed, 4 ignored (213 suites) |
| ownership guard | `cargo test -p gaia-core --test workspace_ownership` | 3/3; fails on a removed row (shown above) |
| ucpu (S1.4 b) | `UCPU_JOBS=2 scripts/ucpu-gate.sh all` (zig 0.15.2) | RESULT ok: grep ok; x86_64 + aarch64 musl release builds static; size-budget ok (27 + 1 binaries per target); armv7-unknown-linux-musleabihf + aarch64-linux-android `cargo check` ok |
| repo-check (S1.4 d) | `cargo run -p repo-check -- --copyright "3000 Galion / galion-studio"` | 1114 files, 0 errors, 29 warnings (all `utc`) -> PASS (before F2: 1 error) |
| no-python | `scripts/no-python.sh` | ok |
| real gaia CLI | `GALION_REAL=1 cargo test --release -p gaia --test real_cli -- --nocapture` | 2/2 (VPS part skipped, no GAIA_VPS): dry-run 52 systems, 52 planned, 5 destructive refused, 0 acted; ownership crates_total 53, system_crates 41, systems_with_a_crate 28 |

Not run here (S1.4 c and the rest of d belong to the verifier fan-out): the 7
real scripts, REUSE lint and `cargo deny`.

## Diffstat

- vs main 13a296c: 506 files changed, 75678 insertions(+), 3844 deletions(-)
- vs integration-1 8fd26f6: 68 files changed, 7207 insertions(+), 671 deletions(-)
- per merge (vs first parent): #1 15 files +1388/-528; #2 14 files +1927/-23;
  #3 25 files +1716/-84; #4 18 files +2118/-40; F1+F2 5 files.

No new third-party packages: Cargo.lock gains no `[[package]]` against
integration-1 (new edges only: gaia -> hades-gate, serde_json; gaia-poseidon ->
openmind-queue, serde_json).

## Open items for Slavik

1. The five `pending Slavik` registry rows above (decided properly in S2.2,
   with the A/B choice for openmind-queue).
2. New `scripts/real/*.sh` arrive with the merged branches
   (gaia-hades-tokens.sh, gaia-poseidon.sh, lan-routing.sh; gaia-live.sh
   changed). lan-routing's commit (20:48) is after the 20:41 Rust-only
   "no new scripts/real/*.sh" rule. Kept as merged; porting them to Rust tests
   is a follow-up.
3. The `gaia-dummy` helper binary (loopback test target for heal-tick, from
   gaia-live, already in integration-1) is built into
   the release musl set; harmless (size-budget ok) but ships in dist unless
   excluded.
