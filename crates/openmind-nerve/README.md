<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# openmind-nerve

The nervous system and cortex of Obsidian Open Mind, made by Galion. One
loopback service (live unit name `galion-nerve`, port 8670), std +
`serde` / `serde_json` + `zagreb-time`; no TLS, no C deps, no async runtime.
Also a library: the JSONL `Writer` / `Event` API that 13a296c shipped, plus
`probe`, `access`, `logscan`, `incident`, `cortex`, `score` and `replay`.
Design notes: [`docs/ws-nerve.md`](../../docs/ws-nerve.md).

```sh
openmind-nerve serve [--listen 127.0.0.1:8670] [--log events.jsonl] [--max-bytes N] [--keep K]
openmind-nerve once [--md]          # one observe-only cycle, print the self-eval
openmind-nerve replay DIR [--md|--state]   # re-run a captured VPS snapshot; exit 0 = pass
```

## What one cycle does (default every 30 s, about 25 ms of CPU)

1. **Ingest**: health endpoint of every component, `systemctl show` batch,
   /proc RSS, `journalctl -o json` (zerolog `ERR`/`WRN` are counted even at
   journald priority 6, which is how cloudflared logs), cloudflared metrics,
   df/meminfo, listening sockets (/proc/net/tcp*), and tails of access logs
   (`NERVE_ACCESS_LOGS=name=path[@component],...`, CLF or JSON lines).
2. **Per-component state** (healthy / degrading / failing-soon / failing),
   trends and projections. A 503 `loading` body older than
   `NERVE_LOADING_GRACE` (900 s) is reported as *stuck loading*.
3. **Cortex**: vulnerabilities (public or unmanaged listeners, scanner
   probes, Vite dev servers on public hostnames), *bleeding* (tunnel
   ingress to closed origin ports with hosts, ports and failed req/min,
   restart loops, memory leaks, error rates, host pressure) and **DDoS**.
   Each detector is a `Sensor` in `src/sensors/` (listeners, scanners, edge,
   components, ddos); findings carry a B41 `kind` and their sensor as `source`,
   and `/api/v1/state` has `cortex_sensors` (per-sensor counts, us) and
   `cortex_events` (B33 `NerveEvent`s). `NERVE_SENSORS_OFF=name,...` turns
   sensors off. Adding one: [`docs/cortex/sensors.md`](../../docs/cortex/sensors.md).
4. **DDoS detector**: 10 s buckets, 2 h kept; the baseline is the median and MAD of
   the window from 30 min ago to 2 min ago. Threshold = max(`NERVE_DDOS_MIN_RPS`,
   5x median, median + 8 x 1.4826 x MAD). Level = flood / elevated / normal,
   and shape = single-source / distributed / concentrated. Streams: each access log,
   the cloudflared request counter, and the nerve's own API (`nerve-api`).
5. **Correlation**: signals are grouped by cause (`group`, `edge`, `ddos:*`,
   `scan:*`, `stack-wide` when 3 or more groups fail hard) into incidents with
   stable ids, a "why" and a "next step". Incidents resolve after 2 quiet cycles.
6. **Score**: 100 minus listed deductions, giving grade A-F. The self-eval passes only
   with no error incident, no error vulnerability, no probe errors and a score of 70 or more.
7. Reflexes (restart of allowlisted units) only with `NERVE_REFLEX=1` and
   the mode switch; observe-only by default.

## API (loopback only)

| route | |
|---|---|
| `GET /` | dashboard (score, incidents, deductions, cortex, DDoS, components) |
| `GET /api/v1/state` | everything below in one JSON |
| `GET /api/v1/{incidents,cortex,ddos,signals,alerts,audit}` | slices |
| `GET /api/v1/self-eval`, `/api/v1/self-eval.md` | scored self-eval |
| `POST /api/v1/signal` | push a signal into the hub |
| `GET /health` | gaia-live compatible: `{"status":"ok","service":"openmind-nerve",...}` |
| `POST /event` | gaia-live compatible JSONL append, `{"ok":true,"appended":n}` |
| `GET /events?n=N` | last N events (default 50, max 1000) |
| legacy | `/state`, `/self-eval(.md)`, `/alerts`, `/audit`, `/signals`, `/healthz`, `POST /signal` |

## Box / test switches

`NERVE_COMPONENTS` (JSON component table), `NERVE_SYSTEMD=0`,
`NERVE_JOURNAL=0`, `NERVE_REFLEX=0|1`, `NERVE_LISTEN_SCOPE=lo-hi`,
`NERVE_KNOWN_PORTS`, `NERVE_PUBLIC_OK_PORTS`, `NERVE_DDOS_MIN_RPS`,
`NERVE_DDOS_MIN_REQUESTS`, `NERVE_EVENT_LOG`, `NERVE_DATA_DIR`, `NERVE_SENSORS_OFF`.

Real-life tests: `GALION_REAL=1 cargo test -p openmind-nerve --test real_box`,
`scripts/real/nerve.sh`, `scripts/real/nerve-capture-vps.sh` (read-only);
results in [`docs/real-tests/nerve.md`](../../docs/real-tests/nerve.md).
