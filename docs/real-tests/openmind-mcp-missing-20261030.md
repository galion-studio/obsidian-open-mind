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

## Live VPS checks (2026-10-30, CET)
- Gate: /var/lib/galion-gate/runs/openmind-mcp-missing-2. Static 5/7 (protected_targets: read-only default paths; no_secrets: fake test tokens and the redactor prefix list) waived by single-use owner exception owner-exception-1.json. Forced stages in unshare -rn on base 498244ec + patch (tree == 61f9c08c): build exit 0, tests 19 passed 0 failed. clippy not installed in the gate toolchain (clean on the box).
- 12:44 CET direct stdio run of the new binary with the live args: tools/list = 18 tools (8 old + 10 new); every new tool answered (stack_health score 91, stack_alerts, cortex_heals, spend_status fresh 3.7302, backup_status ok 12 archives newest 5.4 h, queue_status POSEIDON ok, docs_list/search, hermes_readiness live:false 12/15); docs_read "one" refused (out of scope); vault_note_create "unknown tool" (off). No non-loopback IP and no token pattern in the output.
- 12:46:26 CET one hermes-rs restart (drop-in 99f bind-mounts the new binary over the 498244ec path): health 200, journal "mcp: 18 tools" (was 8), MCP child exe sha256 db99be86 (new).
- 12:48 CET Hermes smoke (model hermes-agent): asked to call mcp__openmind__spend_status; HTTP 200, 10.0 s, answer "The fresh budget is 3.7468 USD and the state is ok." Cost $0.0219 (ledger 7.1792 -> 7.2011).
