<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cortex-sshd fixture (synthetic)

Synthesised on 2026-10-20 23:40 CEST (+02:00) for T2.1 (`sshd` sensor); no host data. Addresses are documentation
ranges (198.51.100.0/24, 203.0.113.0/24, 192.0.2.0/24, RFC 5737); names are made up.

One snapshot at 2026-10-20 23:30:00 +02:00 (`state-0.json`) and `journal.jsonl` (journald JSON, `ssh.service`):

| lines | when | expected |
|---|---|---|
| 100 `Failed password` from 20 sources | 22:56:40 (T-2000 s) | outside the 300 s window: not counted |
| 400 `Failed password for invalid user` from 50 sources | last 280 s | ddos **crit** (`failed_password >= 300`) |
| 35 `Invalid user` | last 200 s | vulnerability warn (`invalid_user >= 20` since T3.1, was 30) |
| 600 `Accepted publickey for root` | last 250 s | counted (`accepted_publickey_root=600`), never a signal |
| 1 `Accepted publickey for fixturedeployer` | T-30 s | vulnerability warn (`accepted_publickey_other >= 1`) |
| 1 `Connection closed` | T-20 s | ignored |
| 1 `Failed password` in `cron.service` | T-10 s | other unit: ignored |

Replay: `NERVE_SSHD=1 openmind-nerve replay tests/fixtures/cortex-sshd` (tests/cortex_s2a_replay.rs and the
GALION_REAL=1 test in tests/real_cortex_s2a.rs).
