<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Steady loopback traffic to hermes-rs :8642 and galion-one :8091

Wave-4 slot 116 (`swarm/vps-traffic-hermes`). READ-ONLY investigation on the VPS,
2026-10-21 02:55–03:08 CEST (+02:00). Tools: `ss` (with `--cgroup`), `/proc`,
`/sys/fs/cgroup/.../cgroup.procs`, `systemctl show/cat`, `journalctl`, `strings`,
GETs to galion-health :8660 and the cloudflared metrics port. No packet capture was
taken. No service was stopped, restarted or changed. Only loopback ports appear below.

## Answer in one line

The traffic comes from our own pollers, not from outside. **galion-one's `sample()` thread
runs once a second**. On every tick it opens one new TCP connection to hermes-rs `/health`
(:8642) and one to **itself** at `/health` (:8091). **link-watch** adds one hermes
`/health` probe every 3 s. Expected total: 8642 ≈ 1.33/s, 8091 ≈ 1.0/s, which matches what
we measured. Nothing is wrong. It is just wasteful.

## Measurements

| What | 8642 (hermes-rs) | 8091 (galion-one) |
|---|---|---|
| Listener | `hermes-gateway`, cgroup `hermes-rs.service`, 127.0.0.1 only | `galion-one`, cgroup `galion-one.service`, 127.0.0.1 only |
| Distinct connections seen in a 60 s tight `ss` loop (covers about a 120 s window because of TIME-WAIT) | 165 → **≈1.4/s** | 123 → **≈1.0/s** |
| New connections in a 28 s window (first-seen timestamps) | 38 (≈1.36/s) | 28 (1.00/s) |
| Gaps between new connections | a regular 1.00 s beat, plus extra arrivals at 0.0–0.7 s offsets (about 10 in 28 s) | almost perfectly 1.00 s (0.98–1.12) |
| Client cgroup of every caught ESTAB/SYN-SENT socket (`ss --cgroup`) | `system.slice/galion-one.service` (9 of 9 caught in 40 s) | `system.slice/galion-one.service` (all caught, 5 in 100 s) |
| TIME-WAIT side | mostly server side (the server closes first, HTTP/1.0 style) | mostly server side |

Cloudflared (the named tunnel that serves `one.galion.app` → :8091) handled only
**8 requests in 60 s for all hostnames together** (`cloudflared_tunnel_total_requests`
went from 94023 to 94031), so external traffic is a rounding error at most. Journals of
`hermes-rs` and `galion-one` are silent: neither logs requests.

## Who, and why (source at the live commit 2f5a730)

1. **galion-one → :8642 and :8091, 1/s each.** `crates/galion-one/src/main.rs`:
   `main()` starts `thread::spawn(|| loop { sample(); thread::sleep(1 s) })`. `sample()`
   starts one thread per spec, `console:8080 /`, `hermes:8642 /health`, `whisper:8643`,
   `voice:mouth(8002)`, `vision:8645`, `one:<own bind> /health`, and makes each a
   fresh `http_to(..., 400 ms)` connection with no keep-alive. 8643 and 8645 have no
   listener, so those fail at once with RST and leave no TIME-WAIT. The tick is
   1 s + the slowest probe (a few ms), which gives the exact 1.00 s beat. The `one`
   probe is galion-one checking its own `/health` over TCP. That handler only returns
   `ok\n`, so the probe proves only that the accept loop is alive.
   - The same tick also runs every 5th tick: `ps -eo args` (process spawn), plus the defence/units
     loaders. Every 15th tick: `load_edge()` spawns 5 `curl` processes
     (`-A galion-nerve`) to the public URLs of one.galion.app, console.galion.app,
     obsidian.galion.app, galion.studio and merlinelement.com, each going out through
     Cloudflare. These were seen live as children in the galion-one cgroup.
2. **link-watch → :8642, 1 per 3 s.** `crates/link-watch/src/main.rs`: a loop of
   `probe(ollama /api/ps)` (nothing listens on 11434, so it fails fast), then
   `probe(hermes /health)`, then a France probe every 30 s, then `sleep(3 s)`.
   `state/link.json` mtime advances exactly every 3 s (checked live).
   Together: 1 + 0.33 = **1.33/s** to 8642, versus about 1.36–1.4 measured.
3. **galion-health is not a steady source.** It probes 8642 and 8091 **only when its
   page or JSON is requested**, with a 5 s cache (`crates/galion-health/src/main.rs` on
   `swarm/health-land`). It adds load only while someone keeps the page open (it
   auto-refreshes every 15 s). This explains the small gap between the 1.0/s we
   measured on 8091 and the earlier ~1.1/s estimate: page views during that
   earlier measurement plus the rare tunnel request.

## Is it a problem?

Not functionally. At this rate the cost is about 150–170 TIME-WAIT sockets at any moment,
2.4 short-lived threads per second in galion-one plus 6 probe threads per tick, a `ps` spawn
every 5 s and 5 `curl` processes every 15 s. That is negligible for a 16 GB box. But it:
- adds noise to any connection-rate or edge-rate signal (cortex T2.12 and the
  DDoS-kind findings should treat these loopback pairs as known baseline);
- makes galion-one shell out (`ps`, `curl`), which the Rust-only and no-shellout work
  (slot 148) wants to remove;
- hairpins 20 requests a minute through Cloudflare just to learn our own edge status.

Side finding (not in scope, read-only): cloudflared reports
`cloudflared_tunnel_request_errors` 55858 out of 94031 total requests since it started
(≈59 %). Checked live at 03:09 CEST: of the 25 ingress origins, only 8671, 8080, 8650
and 8091 have a listener. The other 21 (for example 3010, 4000, 4001, 7681, 8000, 8889) do not, so
requests to galion.app, galion.studio, adriaticpadel.com and similar hostnames fail at the
origin. This is worth a separate ingress clean-up or decision item.

## Proposals (owner decision, nothing applied)

| # | Change | Effect | Risk |
|---|---|---|---|
| P1 | galion-one: drop the TCP self-probe of `one` (report "up" from inside the process, or probe once at start) | 8091: 1.0/s → ~0 | none. The health page still probes 8091 from outside |
| P2 | galion-one: `sample()` interval 1 s → 5 s for service pulses (keep CPU/mem at 1 s if the console needs it) | 8642 from galion-one: 1/s → 0.2/s | console "service up/down" reacts in ≤5 s instead of ≤1 s |
| P3 | link-watch: hermes probe every 15 s instead of every 3 s (keep the ollama/laptop probe at 3 s) | 8642: −0.27/s | link card's hermes field is up to 15 s stale |
| P4 | galion-one `load_edge()`: replace 5 `curl` spawns with a pure-Rust client (slot 148) or read edge status from cortex/cloudflared metrics | no process spawns. Cloudflare hairpin removed if metrics are used | needs the slot-148 outcome |
| P5 | cortex: whitelist the (galion-one → 8642/8091) and (link-watch → 8642) loopback pairs as baseline | cleaner edge-rate/DDoS signals | none |

After P1+P2+P3 the steady rate would be about 0.27/s to 8642 and ~0/s to 8091. These are
code changes in galion-one and link-watch. They need a release and a restart of live units,
which needs Slavik's cutover approval and is outside this read-only slot.

## How to reproduce (read-only)

```
# rate: distinct client ports per server port over 60 s (TIME-WAIT included)
end=$((SECONDS+60)); while [ $SECONDS -lt $end ]; do
  ss -tanH '( sport = :8642 or sport = :8091 or dport = :8642 or dport = :8091 )'; done
# owner: client cgroup of live client sockets
ss -tnH --cgroup state established state syn-sent '( dport = :8642 or dport = :8091 )'
# children spawned by galion-one
cat /sys/fs/cgroup/system.slice/galion-one.service/cgroup.procs
```
