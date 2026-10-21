<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-server record: vps-baseline-2 (slot 182), 2026-10-21 CEST (+02:00)

Docs-only, READ-ONLY row: no build, no binary started, nothing written on the VPS. Evidence = live reads of the production VPS (51.38.137.193) over ssh, `nice -n 10`, `TZ=Europe/Zagreb`.
Secret handling: `Environment=` values were replaced by names on the VPS before output; no EnvironmentFile or secrets dir was read; the output was grepped for key/token/secret/password (only hit: the comment "no secrets" in openmind-queue.service).

| Batch | Time (+02:00) | What | Result |
|---|---|---|---|
| 1 | 11:23:08 | host, list-units, timers, `systemctl show` + ExecStart + exe sha for 17 unit names, `ss -ltnp` | 14 running units, file sha == /proc exe sha for all 14; 3 names not-found (poseidon, galion-cortex, cortex) |
| 2 | 11:23:38 | openmind-queue + galion-nerve-shadow detail, unit files and all drop-ins (env names only), fragment mtimes, /opt/galion/rust/bin sha, /var/backups listing, cloudflared ingress, 29 loopback GETs, nerve /state | 7 new drop-ins, 2 new units, 37 ingress hostnames, codes in §2b of docs/ops/vps-baseline.md |
| 3 | 11:23:59 | cloudflared refusals by upstream, queue paths, galion-deploy/tuner sha, nerve /health and /events, galion-one.log tail | 7994 refusals/h; `/healthz` 200 on 19120; `/events` 1 event |
| 4 | 11:24:22 | queue `/healthz`, `/stats` bodies | `{"function":"POSEIDON","ok":true,"service":"openmind-queue",…}`; `/stats` shows agents deploy-check, deploy-check-b, all zero |
| box | 11:24 | public GET from the box | galion.app 200, www.galion.app 200, one.galion.app 200, tuner.galion.app 000 |
| recheck | 11:25:46 | MainPID, is-active, exe sha prefix, NRestarts for all 14 units, nerve /health | identical to batch 1 (openmind-queue NRestarts 1, tanga-proto 3, rest 0); nerve cycle 204, score 61 |

Nerve (real-time observation, live 127.0.0.1:8670 /state, 11:23:24): `tool openmind-nerve`, started 09:43:54, cycle 200, interval 30 s, mode live,
counts healthy 11 / degrading 1 / failing-soon 1 / failing 0 / absent 2, **score 61, grade D**, 3 open incidents:
galion-one-mouth 503 "loading" (chatterbox), edge "18 public hostnames to 11 closed origin ports … 108.6 failed requests/min", hermes-rs spend cap hit.
The cortex is not a separate unit; its output is the `cortex` / `cortex_events` keys inside the nerve's /state.

Cross-check: every full sha256 in docs/ops/vps-baseline.md was matched by `rg` against the saved probe outputs (9 of 10; the 10th, galion-deploy `2a60e1ac…`, came from batch 3 stdout, quoted above in the ledger).
Shortened sha prefixes were matched against the probe outputs or the 20 Oct ledger (20/20).
