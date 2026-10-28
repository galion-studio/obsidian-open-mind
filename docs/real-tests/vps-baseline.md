<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life evidence: vps-baseline (S1.1)

- This task is a read-only VPS ledger. No binary was built or started on the box, and no port in slot 45's range (19450-19459) was used.
- Live VPS probes:
  - Session 1: 2026-10-20 22:52:32 to 22:52:35 CEST (+02:00), 1216 output lines.
  - Session 2: 22:53:09 CEST (+02:00).
  - Both used `nice -n 10 bash -s` over ssh. Every result is recorded in `docs/ops/vps-baseline.md` §1-§5.
- HTTP results (loopback GET, one each):

  | Code | Endpoints |
  |---|---|
  | 200 | 8091/health, 8646/health, 8650/ and /health, 8642/health, 8420/health, 8431-8433/health, 8652/health, 8653/health, 20241/ready, 20242/ready, 8670/ and /state, 8660/, 8671/tanga/, 8080/package.json |
  | 503 | 8002/health |
  | 404 | 8670/health, 8660/health, 8671/health and /, 8080/health, 8646/ |
  | 302 | 8080/ |
  | 000 (refused) | 4020/health, 8003/health |
- Nothing was restarted, written or POSTed. No secrets directory or EnvironmentFile was read. Env values were withheld (names only).
