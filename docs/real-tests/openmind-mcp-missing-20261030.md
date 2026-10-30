<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# openmind-mcp-stack 0.3.0: stack tools (swarm/openmind-mcp-missing-20261030)

All times CET (Europe/Zagreb, +01:00).

## Box tests (2026-10-30 ~12:40 CET)
- `cargo test -p openmind-mcp-stack`: lib 2, hermes_roundtrip 2, stack 4, stdio 11 — all pass. Galion One tools dropped (scope change t452u); docs tools exclude the `one` folder (tested).
- `tests/stack.rs` runs every new tool against mock loopback nerve/cortex/queue
  services and temp spend/backup/readiness/docs files. Checks: shapes, only GET sent, no key sent to any stack service,
  no IP / token / key in any output, write tool not listed
  by default, write tool never overwrites and leaves every existing note byte-identical.
- `tests/hermes_roundtrip.rs`: the real hermes-rs MCP client lists the tools; `--check`
  shows stack tools by default, refuses unknown conf keys, non-loopback URLs and a
  vague write switch (exit 2).
- clippy `--all-targets -D warnings`, fmt: clean.

## Live VPS checks
Filled in after the deploy (see the swarm report openmind-mcp-missing-20261030.md).
