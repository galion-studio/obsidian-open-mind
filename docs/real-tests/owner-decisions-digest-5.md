<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Evidence for owner decisions digest 5 (slot 449)

Read-only checks on 2026-10-31 09:45 CET (+01:00), no content, addresses or
secrets recorded:

- `systemctl show <unit> -p ExecStart -p ActiveEnterTimestamp -p NRestarts`
  for gaia-cortex, galion-nerve, openmind-memory, hermes-rs: binaries as in
  the digest table, NRestarts 0 on all. VPS clock is UTC; times converted to CET.
- Ancestry on the box (scratch object store from the swarm worktrees):
  `git merge-base --is-ancestor <slot sha> <live build sha>` for every row.
- `systemctl list-timers openmind-mcp-status.timer`: next run scheduled
  every 5 min; status files spend.json / backup.json root:nogroup 0640.
- Nerve /state: self_eval 91 A, edge 0 hosts.
