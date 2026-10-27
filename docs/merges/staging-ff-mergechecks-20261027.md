<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->

# staging-ff-mergechecks (slot 256) — 2026-10-27 Europe/Zagreb (CET +01:00)

Throwaway mergechecks of four mergefix branches onto `swarm/main-staging` **7c95348b**.
**Did not commit to `main-staging` or `main`.** Worktree: `/workspace/swarm/wt/staging-ff-mergechecks` on `swarm/staging-ff-mergechecks`.

## Results

| Branch | Tip | FF onto 7c95348b? | merge-tree --write-tree | Notes |
|--------|-----|-------------------|-------------------------|-------|
| swarm/serde-pin-mergefix | 41373013 | **YES** (ahead 5) | CLEAN tree=dc29a948 | Slot 240 gates already green |
| swarm/shared-tar-mergefix | 4eca4b71 | **YES** (ahead 5) | CLEAN tree=3464a2b9 | Slot 242 gates already green |
| swarm/memory-schema-mergefix | 1919ca84 | **NO** (staging↔tip 472/2, mb=723b46c8) | CLEAN tree=f5d2e7f6 | Needs real merge (not FF); classic marker hit 1 but write-tree clean |
| swarm/no-openrouter-mergefix | 4b835a61 | **NO** (staging↔tip 1/9, mb=7e2a12e1) | CLEAN tree=f841ba72 | Tip already merged 7e2a12e; missing only 7c95348 CI fix commit — merge-tree clean (matches slot 241 throwaway) |

## Throwaway merges (aborted; main-staging untouched)

=== throwaway merge serde-pin onto 7c95348b ===
Updating 7c95348b..41373013
Fast-forward
 Cargo.toml                                  |   9 +-
 crates/bench-banking/Cargo.toml             |   2 +-
 crates/desk-hermes/Cargo.toml               |   2 +-
 crates/galion-bench/Cargo.toml              |   2 +-
 crates/galion-desk/Cargo.toml               |   2 +-
 crates/hermes-bench/Cargo.toml              |   2 +-
 crates/hermes-rs/Cargo.toml                 |   2 +-
 crates/link-watch/Cargo.toml                |   2 +-
 crates/one-set-model/src/lib.rs             |   4 +-
 crates/one-set-model/tests/key_order.rs     |  21 +++
 crates/openmind-memory/Cargo.toml           |   2 +-
 crates/repo-check/Cargo.toml                |   2 +-
 crates/repo-check/README.md                 |   1 +
 crates/repo-check/src/lib.rs                |  97 ++++++++++-
 crates/repo-check/tests/deps_pin.rs         | 161 ++++++++++++++++++
 crates/repo-check/tests/real_serde_order.rs | 254 ++++++++++++++++++++++++++++
 docs/decisions/serde-order-pin.md           | 132 +++++++++++++++
 docs/real-tests/serde-order-pin.md          | 139 +++++++++++++++
 docs/third-party/serde-order-pin.md         |  23 +++
 19 files changed, 847 insertions(+), 12 deletions(-)
 create mode 100644 crates/one-set-model/tests/key_order.rs
 create mode 100644 crates/repo-check/tests/deps_pin.rs
 create mode 100644 crates/repo-check/tests/real_serde_order.rs
 create mode 100644 docs/decisions/serde-order-pin.md
 create mode 100644 docs/real-tests/serde-order-pin.md
 create mode 100644 docs/third-party/serde-order-pin.md
RESULT serde-pin: FF-OK head=41373013
=== throwaway merge shared-tar onto 7c95348b ===
Updating 7c95348b..4eca4b71
Fast-forward
 Cargo.lock                              |   8 +
 crates/gaia-core/src/ownership.rs       |   5 +-
 crates/gaia/tests/commands.rs           |   4 +-
 crates/galion-backup/Cargo.toml         |   4 +-
 crates/galion-backup/src/backup.rs      |   6 +-
 crates/galion-backup/src/tarw.rs        | 367 ++--------------------------
 crates/galion-deploy/Cargo.toml         |   4 +-
 crates/galion-deploy/src/tar.rs         | 221 +----------------
 crates/galion-tar/Cargo.toml            |  14 ++
 crates/galion-tar/src/gnu.rs            | 341 ++++++++++++++++++++++++++
 crates/galion-tar/src/lib.rs            |  32 +++
 crates/galion-tar/src/names.rs          | 102 ++++++++
 crates/galion-tar/src/num.rs            | 138 +++++++++++
 crates/galion-tar/src/read.rs           | 415 ++++++++++++++++++++++++++++++++
 crates/galion-tar/src/ustar.rs          | 218 +++++++++++++++++
 crates/galion-tar/tests/gnu_oracle.rs   | 198 +++++++++++++++
 docs/gaia-ownership.md                  |   5 +
 docs/galion-backup.md                   |   6 +
 docs/galion-tar.md                      |  84 +++++++
 docs/real-tests/shared-tar-crate.md     |  71 ++++++
 docs/real-tests/shared-tar-mergefix.md  |  78 ++++++
 docs/third-party/shared-tar-crate.md    |  17 ++
 docs/third-party/shared-tar-mergefix.md |  10 +
 23 files changed, 1780 insertions(+), 568 deletions(-)
 create mode 100644 crates/galion-tar/Cargo.toml
 create mode 100644 crates/galion-tar/src/gnu.rs
 create mode 100644 crates/galion-tar/src/lib.rs
 create mode 100644 crates/galion-tar/src/names.rs
 create mode 100644 crates/galion-tar/src/num.rs
 create mode 100644 crates/galion-tar/src/read.rs
 create mode 100644 crates/galion-tar/src/ustar.rs
 create mode 100644 crates/galion-tar/tests/gnu_oracle.rs
 create mode 100644 docs/galion-tar.md
 create mode 100644 docs/real-tests/shared-tar-crate.md
 create mode 100644 docs/real-tests/shared-tar-mergefix.md
 create mode 100644 docs/third-party/shared-tar-crate.md
 create mode 100644 docs/third-party/shared-tar-mergefix.md
RESULT shared-tar: FF-OK head=4eca4b71
=== throwaway merge memory-schema onto 7c95348b ===
Committer identity unknown

*** Please tell me who you are.

Run

  git config --global user.email "you@example.com"
  git config --global user.name "Your Name"

to set your account's default identity.
Omit --global to set the identity only in this repository.

fatal: unable to auto-detect email address (got 'box@grok-bot-vm-303083841.(none)')
RESULT memory-schema: MERGE-CONFLICTS
=== throwaway merge no-openrouter onto 7c95348b ===
Committer identity unknown

*** Please tell me who you are.

Run

  git config --global user.email "you@example.com"
  git config --global user.name "Your Name"

to set your account's default identity.
Omit --global to set the identity only in this repository.

fatal: unable to auto-detect email address (got 'box@grok-bot-vm-303083841.(none)')
RESULT no-openrouter: MERGE-CONFLICTS
After throwaways DEST=7c95348b main-staging=7c95348b

## Prior full gate citations

- **serde-pin 4137301** (slot 240 DONE 2026-10-22 01:30 +02:00): fmt ok; clippy -D workspace (excl sandbox/cgroup-detect) clean; workspace tests 370 suites 1966/0/18; repo-check 0 err/79 warn; GALION_REAL `real_serde_order` 3/3 on :21100; throwaway ff-only onto staging copy → 4137301.
- **shared-tar 4eca4b7** (slot 242 DONE 2026-10-22 01:46 +02:00): 370 suites 1958/0/18; repo-check 0/79; GALION_REAL `gnu_oracle` 2/2 + `real_parity` 26/26; descendant of 7c95348b → FF.
- **memory-schema 1919ca8** (slot 239 DONE, LOCAL at time): merge-tree was reported clean onto 7c95348 in migration resume; this re-check confirms write-tree CLEAN but **not** fast-forward (branch diverged before staging's later history).
- **no-openrouter 4b835a6** (slot 241 DONE 2026-10-22 01:43 +02:00): throwaway test-merge onto 7c95348b produced clean tree f841ba72; 369 suites 1964/0/18; GALION_REAL `real_no_openrouter` 2/2. Re-check: still merge-tree CLEAN; not FF because tip's merge-base is 7e2a12e (one staging commit behind).

## Owner / parent next steps

1. FF-merge (or `git push` then FF on staging) **serde-pin** + **shared-tar** when PR #12 CI allows staging movement.
2. Non-FF **memory-schema** + **no-openrouter**: merge (not FF) onto staging in a dedicated integrator; expect clean trees per merge-tree.
3. Do **not** land on `main` until PR #12 green (main still 13a296c).

Ports 21250-21259 unused (docs/mergecheck only).

## Correction (2026-10-27 17:36 CET +01:00)

The first throwaway `--no-ff` attempts for **memory-schema** and **no-openrouter** failed with `Committer identity unknown` (no `user.email` in that clone), which was mis-logged as MERGE-CONFLICTS. Re-run with `user.email=swarm@galion.local` / `user.name=swarm-256` and `--no-commit`:

- memory-schema: MERGE-OK (no conflicts), would stage 12 paths; aborted, nothing committed.
- no-openrouter: MERGE-OK (no conflicts), would stage 12 paths; aborted, nothing committed.
main-staging still 7c95348b
