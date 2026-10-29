<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: hermes-tools-restore-20261029

- Branch: `swarm/hermes-tools-restore-20261029` (on top of helper branch f67d9d45).
- Box run: 2026-10-29, 23:00-23:59 (Europe/Zagreb, CET +01:00). No VPS change, no deploy, $0 OpenRouter spend.

## Box (local)

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 2350 passed, 0 failed, 19 ignored (23:49 CET) |
| `hermes-tools --test agent_tools` | 16 passed (36 terminal attacks refused by the deny list; layer-2 sandbox holds without the deny list; staged skills load) |
| `hermes-rs --test denylist_bypass` | 26 passed, 1 ignored |
| opt-in real browser (`HERMES_RS_TEST_BROWSER=1`, chromium_headless_shell-1248) | example.com text + screenshot ok; vision path ok (mock provider); loopback via localtest.me blocked by PAC, 0 hits on the listener |
| release musl `hermes-gateway` (zig cc + rust-lld) | 2,441,216 B, stripped ELF; under the 200 MB budget (t416u), over the old 2 MiB |

The terminal and browser sandbox runs above used the local box kernel (user namespaces on).
RULES.md wants every sandbox run on the VPS as well; see the VPS section.

## VPS sandbox

Not run yet in this note; see the branch report.
