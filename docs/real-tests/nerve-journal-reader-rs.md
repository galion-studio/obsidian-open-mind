<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: nerve-journal-reader-rs

**When:** 2026-10-27 evening Europe/Zagreb (CET +01:00)  
**Slot:** 282 · branch `swarm/nerve-journal-reader-rs` · base `bb287cc`  
**Ports:** 21520-21529 · **NO deploy · NO VPS writes**

## What was tested

1. **Research** of MIT/Apache journal readers → chose `sdjournal` 0.1.22 (see `docs/third-party/nerve-journal-reader-rs.md`).
2. **Feature-flagged** in-process follower in `crates/openmind-nerve/src/evt/journal_native.rs` (`journal-native` + `NERVE_EVT_JOURNALCTL=native`).
3. **Unit tests** (`fields_to_value` → existing `parse_entry`).
4. **Box RSS comparison** (`tests/journal_native_rss.rs`).
5. **Read-only VPS probe** of the live journalctl child under `openmind-nerve-bb287cc`.

## Live VPS (read-only) — 2026-10-27 ~17:55 CET (+01:00)

| process | PID | RSS |
|---|---|---|
| `openmind-nerve-bb287cc serve --mode event` | 308 | **8088 KB (~7.9 MB)** |
| `journalctl -f -o json …` (ppid 308) | 541 | **50264 KB (~49.1 MB)** |

Inventory item 9 called this ~45 MB; tonight’s sample is ~49 MB. Nerve itself stays small; the C child dominates.

## Box RSS comparison

Commands:

```bash
export PATH=/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/nerve-journal-reader-rs
export CARGO_INCREMENTAL=0
cargo-slot test -p openmind-nerve --features journal-native --test journal_native_rss -- --nocapture
```

Measured on the box 2026-10-27 ~18:00 CET (+01:00):

| metric | value |
|---|---|
| box `journalctl -f` RSS | **7444 KB** |
| nerve replay-mode RSS | **10932 KB** (0 journalctl children) |
| nerve native-mode RSS | **10904 KB** (0 journalctl children) |
| VPS live nerve RSS | **8088 KB** |
| VPS live journalctl child RSS | **50264 KB (~49.1 MB)** |

Raw dump: `docs/real-tests/nerve-journal-reader-rs.rss.txt`.

On the VPS, removing the journalctl child saves ~49 MB RSS per nerve
instance (owner deploy decision). Box native ≈ replay (no journal files to map).

Expectations:

- Native mode spawns **zero** `journalctl` children.
- Native nerve RSS stays in the same order as replay-mode nerve (no +45–50 MB child).
- On the VPS, flipping live to native is an **owner decision** (see third-party doc).

## Unit / crate tests

```bash
cargo-slot test -p openmind-nerve --features journal-native journal_native -- --nocapture
cargo-slot test -p openmind-nerve --lib
```

## Open issues

- Box has no `*.journal` files; native path is exercised via `native:<empty-dir>` (idle follower) + field-conversion unit tests. Full golden parity vs journalctl JSON needs a host with journals (VPS deploy rehearsal — owner).
- `set_units` restart for native uses a 500 ms `recv_timeout` poll (no SIGKILL of a child); inventory churn may lag ≤500 ms.
