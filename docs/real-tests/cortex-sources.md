<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life record: cortex-sources (slot 44, ports 19440-19449)

Docs + fixture task; no new code. Two kinds of evidence.

## 1. READ-ONLY VPS probes (2026-10-20 22:52:48 - 22:53:19 CEST, +02:00)

Three ssh sessions (`-i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes`), VPS load 3.96 / 3.57 / 3.26 at start.
Commands: `systemctl list-units --type=service|timer --all`, `ss -ltnp`, `ss -lunp`, `curl -s -m 5` GET on loopback ports,
`ps -o pid,ppid,user,etime,comm`, `/proc/<pid>/cgroup` (first line), `systemctl show galion-nerve -p ActiveEnterTimestamp,User,FragmentPath`,
`grep -l NERVE_URL /etc/systemd/system/*.service` (file names only: none). No POST, no restart, no secrets or Environment read.

| GET | result |
|---|---|
| 8420/health, 8431/health, 8432/health, 8433/health | 200 (278/278/278/277 B) |
| 8642/health, 8652/health, 8653/health | 200 (122 B each) |
| 8646/health, 8650/health, 8091/health | 200 |
| 8002/health | 503 (86 B) |
| 8080/ | 302 |
| 8660/status.json | 200 (1394 B, 0.277 s); lists 12 services |
| 8670/healthz, /state, /self-eval.json, /signals, /alerts | 200; `/signals` = `{"rejected":0,"sources":[]}` |
| 8670/api/v1/*, /events, /incidents, /cortex | 404 (old build a63b730 has no v1 API / cortex) |
| 8671/, /health | 404; 8671/tanga/ 200 (41547 B) |
| 20241/metrics, 20242/metrics | 200 (26954 B, 17806 B) |

Live nerve `/state` (22:53:19 CEST): 15 components, `connection` = `pull` x14 + `self`; system `degrading`
(11 healthy, 2 degrading: galion-one-mouth HTTP 503, cloudflared "tunnel origin errors 174/478 requests in 10 min";
2 absent: voice-worker, gaia). `/self-eval.json` `pass: true`. Started 2026-10-18 22:35:04 CEST, `User=root`.

## 2. Box run of the integration-1 nerve with the fixture (2026-10-20 22:56:03 CEST)

```
CARGO_TARGET_DIR=/workspace/swarm/target/cortex-sources CARGO_INCREMENTAL=0 nice -n 10 cargo build --release -p openmind-nerve -j 2
NERVE_ADDR=127.0.0.1:19440 NERVE_INTERVAL=5 NERVE_COMPONENTS=crates/openmind-nerve/tests/fixtures/nerve-components-vps.json \
NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_GALION_HEALTH_PORT=0 NERVE_REFLEX=0 NERVE_DRY_RUN=1 NERVE_LISTEN_SCOPE=19440-19449 \
  openmind-nerve serve        # then GET /api/v1/state after 8 s, then kill
```

Result: fixture parsed, **20 components** (18 rows + `lxc-host` + `galion-nerve` appended), every row carries its
Gaia function/system as written (e.g. `galion-health MINERVA/guardian-watchdog`, `ssh MINERVA/unmapped:host-security`).
Status on the box (no VPS ports there): 7 failing (HTTP rows), 2 degrading (non-critical HTTP rows: mouth, tanga-proto),
1 absent (voice-worker), 10 healthy; verdict `failing`, score 61, grade D, incident "stack-wide failure: 8 components
down at once". Process killed; port 19440 free afterwards (`ss -ltn | grep -c 19440` = 0).

Finding: `cloudflared`, `tuner-quick`, `ssh`, `fail2ban`, `firewall`, `journald`, `link-watch-rs`, `galion-backup` were
`healthy` with `unit_state: null`, `http: null`, no metrics and empty `probe_errors`: a component that yields **no
evidence** is reported healthy (sources.md §5 item 5).
