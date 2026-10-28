<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-health v0.2.0: fixes for health-land findings 1 and 2

Wave 4, slot 149, branch `swarm/health-fixes` on `swarm/health-land` f4a66ea. Times are Europe/Zagreb CEST (+02:00).
The proven v0.1.0 binary stays on `swarm/health-land` f4a66ea, which this work did not change. The live VPS binary
(sha256 `6afe668c...`, rebuilt byte-identical from e43d7ab) is also unchanged. This branch is a NEW version (crate `0.2.0`, `--version`).

## What was wrong (seen on the live 8660, 2026-10-21 06:54 +02:00, read-only GETs)
1. **Split-request reads.** v0.1.0 does one `read()` and closes. On the live service, `GET /stat` + `us.json ...` sent as 2 writes
   returned **404**, `GE` + `T /healthz` returned the **/ HTML page** instead of `/healthz`, and every second write failed (the
   server had already closed). The kernel sends RST when the server closes with unread bytes (`Connection reset by peer`).
2. **One thread did everything.** The accept loop read the request (up to 3 s per idle client), then ran all 12 checks one
   after another on the request path. With a hung upstream (1.5 s connect + 2.5 s read per check) every client waits up to
   about 48 s. Box measurement with 12 hung upstreams: **worst 30.6 s** for 10 clients, a 404 waited 29.6 s, two idle clients
   delayed a third by 6.2 s.

## What v0.2.0 does
| area | v0.1.0 | v0.2.0 |
|---|---|---|
| request read | one `read()` of 1 KiB | reads until the blank line, max 8 KiB head (431 above), 3 s **total** deadline (v0.1.0: 3 s per read) |
| sloppy client (request line, no blank line) | answered | answered on EOF at once, or after the 3 s deadline |
| close | `close()` with unread bytes -> RST | `shutdown(Write)`, then drains up to 64 KiB / 500 ms -> clean FIN |
| threads | 1 | main (accept) + 3 workers (`GALION_HEALTH_WORKERS`, 1-6) + 1 refresher = **5** |
| probes | serial, on the request path | one round in the refresher thread; all HTTP probes run side by side on non-blocking sockets from that one thread; round <= about one read timeout (2.5 s), not the sum |
| systemctl | one process per unit (9) per round | one `systemctl is-active u1 .. u9` (one state per line), 3 s timeout; falls back to one call per unit if the line count is wrong |
| cache | 5 s, refreshed inline | 5 s, single-flight refresh. A request waits for the round up to 4 s if older results exist (then serves those, `checked_at` shows their age), up to 10 s on a cold start (then `503 warming up`) |
| 404 | ran the probe round first | answered without touching the cache |
| overload | accept queue only | 32 queued connections, beyond that `503 busy` at once |
| garbage / no request line | served `/` | `400 bad request` |
| empty connection (connect + close) | ran probes, wrote `/` | closed silently |
| idle connection | served `/` after 3 s | `408 request timeout` after 3 s |

Unchanged: endpoints (`/status.json`, `/json`, `/healthz`, `/`, 404 elsewhere), JSON key set and order (nerve reads
`galion_health_ok` from it), HTML, headers, env vars, check spec, probe timeouts (1.5 s connect, 1.5 s write, 2.5 s read),
`--once`, log format, `unit_state ""` without systemd (finding 3, out of scope), std only (no new dependencies).

## Why this shape (the live unit's limits)
`deploy/foundation/galion-health.service` sets **TasksMax=8**, MemoryMax=16M, CPUQuota=10%. A thread per connection or a thread
per probe would hit TasksMax (thread spawn fails), so v0.2.0 uses a fixed pool and probes from one thread:
5 threads + at most 1 systemctl child = 6 tasks. Measured: 5 threads, VmRSS 532 kB (musl) / about 2.2 MB (glibc), 256 KiB stacks.

Known limit: the 3 workers can all be waiting for one probe round; a 4th request (even a 404) then waits for a free worker,
at most one round (measured 1.5 s with 12 hung upstreams). That is the price of staying under TasksMax=8.

## Not done here (owner decisions)
- **Deploying v0.2.0 to the VPS** (replacing the proven v0.1.0 binary at /opt/galion/rust/bin/galion-health) is Slavik's call.
  Rollback = put back the e43d7ab binary (sha256 `6afe668c...`), which rebuilds byte-identical. The unit file needs no change.
- The behaviour changes in the table (400/408/431/503 busy/503 warming up, stale-while-refresh up to 4 s) need a yes.
- Finding 3 (`unit_state ""` without systemd) and finding 4 (`/status` vs `/status.json` naming) are left as they were.
