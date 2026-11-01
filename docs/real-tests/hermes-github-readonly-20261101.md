<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# GitHub read-only first (2026-11-01, CET)

Config only, no code change: `deploy/staged/hermes-mcp/github-readonly-core.yaml` (staged, not installed).
Checked on the box with the real `hermes-gateway --check-mcp` (base 4ce482de = live 46877fd8 + test-only fix),
a placeholder (fake) token file, no GitHub call:
- core profile: 17 tools, all reads, no write tool listed (~14.7 KB schemas, ~3.7K prompt tokens per call).
- all reads (no allow_tools): 51 tools (~43 KB, ~10.8K tokens).
- live-like (write_tools gated, no allow_tools): 75 tools, 24 gated (~64.5 KB, ~16K tokens).
- no token file: "server github NOT CONNECTED" (0 tokens), as live today.
Needs: Mate's read-only fine-grained token in /etc/hermes-rs/mcp.d/github.env (0600), the entry swapped in
/etc/hermes-rs/mcp-servers.yaml (COMMIT = the live github-mcp-rs build), one Hermes restart.
