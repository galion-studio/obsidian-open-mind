<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests — release-artifacts-matrix (slot 275)

Europe/Zagreb CET +01:00, 2026-10-27 ~17:52.

## Box probes (ports 21450–21454, x86_64-musl artifacts)

| Service | Port | Request | Result |
|---|---|---|---|
| openmind-nerve-7c95348 | 21450 | GET /health | **200** 146 B status ok |
| tuner-7c95348 | 21451 | GET /health | **200** 12 B {"ok":true} |
| galion-health-7c95348 | 21452 | GET / | **200** 559 B HTML |
| openmind-queue-7c95348 | 21453 | GET /healthz | **200** 117 B ok POSEIDON |
| gaia-cortex-73583ab | 21454 | GET /health | **200** 239 B mode dry-run |

Processes killed after each probe; no VPS writes; no deploy.

## --version smoke (all 10 x86_64 bins)

Stamped (commit + built): galion-desk, galion-one, link-watch, openmind-memory, tuner.
Unstamped version string: gaia-cortex, galion-health, openmind-nerve, openmind-queue.
hermes-gateway: `--version` still starts and refuses without API_SERVER_KEY (D-228-4).

Full log: `/workspace/swarm/artifacts/release-20261027/logs/real-probe.log`.
