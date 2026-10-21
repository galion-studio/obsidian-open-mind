<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# integration-5 merge report (wave-4 slot 177)

2026-10-21, Europe/Zagreb (CEST +02:00). Branch `swarm/integration-5`, base integration-4 `42de672`.
Main (`13a296ce`) is an ancestor, so merging this branch into main would be a fast-forward with no textual conflicts.

## Merges (all `--no-ff`, first parent = integration line)

| Commit | Merged | What it brings |
|---|---|---|
| `5cf2e8c` | candle-zip-rebase `b246f66` (= integration-4 + candle-zip-fix `a4f49b5`) | vendored candle-core 0.9.2 on zip 9.0.5 (RUSTSEC-2026-0367/0368/0369) |
| `f917078` | nerve-wiring-rebase `8046c41` (= integration-4 + nerve-wiring-2 `d97dab9`) | nerve-client in hermes-gateway / hermes-rs / openmind-memory, owner map, cortex incident owners |
| `c16639f` | deny-paste-replace `4bfffe8` (on candle-zip-fix `a4f49b5`) | `paste` -> vendored shim over `pastey` 0.2.3 (RUSTSEC-2024-0436) |

Conflicts: **none**. The two rebase branches had already resolved their conflicts against integration-4
(slots 154 and 155), and deny-paste-replace sits on the same candle-zip-fix commit candle-zip-rebase merged.
Checked: `git merge-tree --write-tree` of each pair gives exactly the tree of the merge commit
(`0d94440`, `e9aad16`, `803cf33`), so no hand edits were mixed into the merges. `Cargo.lock` merged textually
(+31/-4, +2, +9/-2 lines) and `cargo metadata --locked` accepts it (no lock rewrite needed).

One follow-up commit on top: `real_wiring` gets `REAL_WIRING_PORT_BASE` (default 20250, unchanged behaviour) so this
slot could run it on its own range 20470-20479. Test-only.

## Results (details: docs/real-tests/integration-5.md)

- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok
  (only the 4 deprecation warnings inside vendor/candle-core, not a workspace member).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: **1241 passed, 0 failed, 6 ignored** (263 suites).
- `cargo deny --all-features check advisories bans licenses sources` (repo deny.toml, cargo-deny 0.20.2): **advisories ok, bans ok, licenses ok, sources ok**.
  Same command on integration-4 `42de672`: **advisories FAILED** (RUSTSEC-2024-0436 paste, RUSTSEC-2026-0367/0368/0369 zip).
- `scripts/no-python.sh`: ok.
- `GALION_REAL=1 real_wiring` (release binaries, ports 20470-20478): **2/2 passed**.
- Release `x86_64-unknown-linux-musl` 37/37 static; `aarch64-unknown-linux-musl` 37/37 static (zig cc, per-shell env);
  `cargo check --workspace --all-targets` armv7-unknown-linux-musleabihf ok, aarch64-linux-android ok.

## Main-merge blockers left

Textually none (fast-forward). What still stands between this branch and main:

1. **Owner decisions carried in from the merged branches** (none decided here):
   - C1 keep vendored, patched `vendor/candle-core` until upstream candle uses zip >= 9 (recommended (a)); C2 upstream issue/PR to huggingface/candle = publishing, yes/no (docs/security/candle-zip-fix.md).
   - Keep the `vendor/paste-pastey` shim (drop when gemm/pulp leave `paste`) (docs/security/deny-paste-replace.md).
   - nerve wiring (slot 155): galion-common alert hook, golden-test exception, and the hung-nerve slowdown question
     (nerve-client connect/write timeout under a SIGSTOPped peer) before any deploy (docs/real-tests/nerve-wiring-rebase.md).
   - Older open items from integration-3: 15 `pending Slavik` ownership rows; heal plan v2 wording (F1); galion-desk, room,
     link-watch not on the galion-common strict HTTP limits (docs/merges/integration-3.md); strict request handling approvals
     H1-H3, M1-M3, D1-D4, R1-R4, C1-C5 (swarm/common-migrate-2).
2. **Finished branches that are not in integration-5** (main would not have them). Merge-tree against this head:
   - clean: nerve-t36-consumer `334beaa`, mouth-loading-diag `a68ad3e`, voice-mouth `5d776c1`;
   - Cargo.lock only: console-port-merge `508401d`;
   - Cargo.lock + crates/galion-one/Cargo.toml: one-no-shellout `40454f9` — **this is what runs live on the VPS since 08:39**,
     so main without it would be behind production;
   - 2 test files: cortex-merge-all `6bcb882` (crates/openmind-nerve/tests/cortex_replay.rs, tests/support/mod.rs);
   - 7 files: hermes-bench-e2e `2b11a5b` (.gitignore, Cargo.lock, gaia-core ownership.rs, gaia tests/commands.rs,
     hermes-gateway Cargo.toml + main.rs, docs/gaia-ownership.md).
   Suggested next integration (integration-6): these six on top of integration-5.
3. **Not run on this branch**: sandbox crates (openmind-sandbox, cgroup-detect, hermes-tools sandboxed tests) - VPS-only rule,
   their code is unchanged by these three merges; GALION_REAL voice tests (real_chatterbox, real_voice_serve, real_voice_rust):
   their fixed ports (19063, 19371-19375) are outside this slot's range, and the voice crates plus vendor/ are byte-identical
   to deny-paste-replace `4bfffe8` (only candle-core GALION-PATCH.md and 2 test .pt fixtures differ), which passed them 07:38-07:46.
4. **aarch64 SQLite cross-compile still needs per-shell env** (slot 156's open gap): `CC_aarch64_unknown_linux_musl="zig cc -target aarch64-linux-musl"`
   fails because cc-rs adds `--target=aarch64-unknown-linux-musl`; it builds with `CRATE_CC_NO_DEFAULTS=1` plus explicit
   `CFLAGS_<target>="-O2 -fPIC -ffunction-sections -fdata-sections -fno-sanitize=undefined"` (without -O2, zig defaults to UBSan and the link fails).
