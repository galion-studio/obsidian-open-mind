<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Discovery seeds

- DESK-ID: openmind.SEEDS-DOC
- Written: 2026-10-19T21:04:58+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Source: `crates/openmind-discovery` and `crates/openmind-route` only. Neither crate was edited. No packet was sent.

Four pending seed ids. Discovery sets `approval` to `pending` on every seed. The host is loopback `127.0.0.1`. Route keeps the same ids and the same loopback endpoints, and it does not choose a pending row.

| id | loopback endpoint |
| --- | --- |
| seed-openmind | 127.0.0.1:7787 |
| seed-ollama | 127.0.0.1:11434 |
| seed-llama-rpc | 127.0.0.1:50052 |
| seed-exo | 127.0.0.1:52415 |

Discovery builds these in `static_seeds`. Ports are `7787` (`PORT_OPENMIND`), `11434` (`PORT_OLLAMA`), `50052` (`PORT_LLAMA_RPC`), and `52415` (`PORT_EXO`). `host` and `addrs` are `127.0.0.1`. Route copies them in `PENDING_SEEDS` as `127.0.0.1:7787`, `127.0.0.1:11434`, `127.0.0.1:50052`, and `127.0.0.1:52415`.

Updated 2026-10-20 (swarm/lan-discovery): `Scan::Requested` (`--scan`) now calls `active_scan`, which runs one real mDNS browse (a query plus one repeat, so `packets_sent` is 2) and adds what answers as pending candidates. `Scan::Off`, the default, still opens no socket. See docs/lan-discovery.md.

The stamp offset is numeric. It is not a trailing UTC designator.
