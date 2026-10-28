<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real checks: docs-status-refresh (wave-4 slot 231)

Docs-only branch `swarm/docs-status-refresh` on integration-5 0d68f46. Plan
items D10, D11, D14, D17 (`/workspace/swarm/plans/docs-whitepaper.md:62-69`,
box file). Times Europe/Zagreb, CEST (+02:00), 2026-10-21. No builds except
`repo-check`; no ports opened. VPS: read-only GETs and `systemctl is-active`
only.

| When | Command | Result |
|---|---|---|
| 23:48:30 | `scripts/status-percent.sh` (base) | items 89 (D 54, P 12, N 2, O 21), weight 122; 60.7 % / 59.0 % done |
| 23:48:30 | `scripts/spdx-scan.sh` | scanned 526, missing 0 |
| 23:48:30-23:48:51 | `scripts/public-export.sh --ref 0d68f46 --allow-missing-tools` (comply 0.1.1, cargo-deny from `/workspace/swarm/tools/bin`) | 1 FAIL (gate 7 REUSE, 14 problems in 7 files), 1 WARN (gate 10: 27 IPv4 literals in 17 files, 1 quick-tunnel host); gates 6, 9 SKIP (tools missing) |
| 23:49:31 | same, `--ref 8fd26f6` (integration-1) | 0 FAIL, 1 WARN; gate 7 1053 total, 1007 passed, 0 failed, 46 skipped; gate 10 20 IPv4, 1 quick-tunnel |
| 23:50 | comply lint on the integration-5 export copy | 1502 total, 1431 passed, 7 failed, 64 skipped |
| 23:50:33 | GitHub branch listing `galion-studio/obsidian-open-mind` (read-only API, 3 pages) | 239 branches, 212 `swarm/*`; `main` 13a296c |
| 23:53:39 | VPS `curl 127.0.0.1:8670/healthz`, `/state`; `127.0.0.1:8674/health`; `systemctl is-active` | nerve ok, mode live, cycle 9 since 22:56:55, score 66 D; cortex ok, mode live, llm_enabled true, hermes_handoff true, incidents_open 0; galion-nerve, gaia-cortex, hermes-rs, openmind-memory active |
| 23:54:46 | `scripts/public-export.sh --ref 79ee761` (launch-gates) | 0 FAIL, 1 WARN; gate 7 724/684/0/40; gate 10 7 IPv4, 1 quick-tunnel |
| after edits | `scripts/status-percent.sh` | unchanged (no status changed; B42 stays P, see STATUS.md) |
| after edits | `scripts/no-python.sh` | no-python: ok |
| 23:55:38-23:55:59 | `cargo-slot run -j 2 -p repo-check -- --copyright "3000 Galion / galion-studio"` | 1519 files, 10 errors, 48 warnings -> FAIL, the same as base (`docs/real-tests/integration-5.md:18`); all 10 errors are `portable` in vendor/candle-core, none in files this branch touched |
| 23:56:22 | `scripts/public-export.sh --ref HEAD` (this branch) | 1520 / 1612 files; gate 5 0 hits; gate 7 FAIL 14 problems (same 7 files as base); gate 10 27 IPv4, 1 quick-tunnel (unchanged: no new disclosure) |

The IPv4 literals and the quick-tunnel host were counted, not copied here.
Cross-check of the branch tables: 7 record lines picked by hand
(CALL-RECORD.md:11, 150, 540, 690, 736, 773, 795) name the same SHA as the
GitHub head in the table.
