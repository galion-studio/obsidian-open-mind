<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console.galion.studio / dashboard.galion.studio "TLS failure" (F-396-4) — slot 414

Read-only diagnosis, 2026-10-28 20:19-20:22 CET (+01:00). Base
`swarm/console-admin-503-diag` 2561a182. No DNS, tunnel, unit or service
change was made. No addresses or secrets are recorded here.

## Answer in one line

These two names **have no DNS records at all** (NXDOMAIN at the authoritative
Cloudflare zone). The "TLS failure, curl exit 35" seen in slot 396 is an
artefact of this box's egress proxy, not a Cloudflare edge or certificate
problem. They are **not live and were never live under the current tunnel**;
they are leftovers in the old TypeScript console's host/fleet lists and in a
Caddy compose template from the pre-tunnel layout.

## Evidence

### 1. Public DNS (DNS-over-HTTPS, public resolvers, 20:19-20:21 +01:00)

| Name | Cloudflare 1.1.1.1 | Google 8.8.8.8 | Records |
|---|---|---|---|
| console.galion.studio | Status 3 (NXDOMAIN), SOA elaine.ns.cloudflare.com serial 2418410416 | Status 3 | none (A, AAAA, CNAME) |
| dashboard.galion.studio | Status 3 (NXDOMAIN), same SOA | Status 3 | none (A, AAAA, CNAME) |
| galion.studio / www.galion.studio | Status 0 | – | Cloudflare proxied A+AAAA |
| console.galion.app (control) | Status 0 | – | Cloudflare proxied A+AAAA |

The same template family is also NXDOMAIN: docs., play., mc., api.galion.studio
and sentialink.com. forge.galion.studio and status.galion.app do resolve.

### 2. curl -v from the box (20:20:12 +01:00)

```
console.galion.studio            Client hello -> TLS alert decode error -> unexpected eof   exit=35
dashboard.galion.studio          same                                                        exit=35
nonexistent-zz9-<epoch>.galion.studio (random, surely unregistered)  same                    exit=35
galion.studio                    TLS 1.3 ok, CN=galion.studio, HTTP 502 (origin :4001 closed)
console.galion.app               TLS 1.3 ok, CN=console.galion.app
```

The box resolver (198.18.0.53) is a fake-IP egress proxy: `getent ahosts`
returns 198.18.0.1 for **any** name, including names that do not exist. The
proxy then drops the TLS handshake for names it cannot resolve upstream, which
curl reports as exit 35. A random non-existent subdomain fails byte-for-byte
the same way, so exit 35 here means "does not resolve", not "bad edge TLS".
Lesson for future probes from this box: always check public DoH first
(F-414-2).

### 3. VPS tunnel ingress (read-only, 20:20-20:21 +01:00)

* `cloudflared.service` active, runs
  `--config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`
  (file mtime 2026-10-21 15:08:54 +00:00 = 17:08:54 CEST, 3420 B).
* 38 ingress `hostname:` entries. `.galion.studio` ones: `galion.studio`,
  `www.galion.studio`, `forge.galion.studio` (added 2026-10-21 for the box
  reverse tunnel). Plus `studio.galion.app`.
* **No `console.galion.studio`, no `dashboard.galion.studio`.**
* `journalctl -u cloudflared --since -7d`: 0 lines mentioning either name.
* Live `console-rust-primary.service` has no `--console-host` flag for them.
* galion-infrastructure git history (`log -S`): neither name was ever in the
  tracked tunnel config.

### 4. Where the names come from

* `/opt/galion/console-ide/src/lib/host.ts` `CONSOLE_HOSTS` (ported 1:1 into
  `crates/console-ui/src/gate.rs` `CONSOLE_HOSTS`).
* `/opt/galion/console-ide/src/lib/fleet.ts`: console.galion.studio = "Console
  IDE alias", dashboard.galion.studio = "Operator console", and the SERVICES
  record `console.hosts = ["dashboard.galion.studio"]` (repo
  galion-initiative-web; mirrored in `crates/console-static/tests/ui.rs`).
* `/opt/galion/console-ide/src/lib/compose.ts`: a Caddy block
  `... dashboard.galion.studio, sentialink.com { reverse_proxy control:8080 }`
  — the older Docker/Caddy layout, superseded by the cloudflared tunnel.
  Its sibling names (docs/play/mc/api.galion.studio, sentialink.com) are all
  NXDOMAIN too.

So they were **intended aliases of the console** in a planned/old layout, but
nothing in DNS or the current tunnel ever served them.

### 5. Nerve observation (VPS 127.0.0.1:8670/state, 20:21 +01:00)

Score 68, grade D, console.service active/running :8080 with 0 errors. The open
edge incident inc-94f3b84e ("tunnel routes 17 public hostnames to 9 closed
origin ports … :4001 …", ~61 failed req/min) covers galion.studio /
studio.galion.app 502s; it does **not** involve the two names here (they never
reach the tunnel). Cortex is live (58a12e6); this slot only read the nerve.

## Findings

* **F-414-1:** console.galion.studio and dashboard.galion.studio are NXDOMAIN
  and absent from tunnel ingress; not live, nothing broken at the edge. F-396-4
  should be re-worded from "fail TLS at the edge" to "not in DNS".
* **F-414-2:** curl exit 35 from this box is the generic symptom of an
  unresolvable name (fake-IP egress proxy). Probes must check public DoH first.
* **F-414-3:** stale host lists: `CONSOLE_HOSTS` in console-ui gate.rs, the
  fleet/SERVICES records and the Caddy compose template still advertise the
  dead names; the console UI shows "dashboard.galion.studio" as the console's
  host, which is misleading for operators.
* **F-414-4 (side note, already tracked):** galion.studio, www.galion.studio
  and studio.galion.app answer 502 (origin :4001 closed) — part of the nerve
  edge incident and tunnel-audit rows 11/19-20, not this slot.

## Owner decision (proposal) — see docs/decisions/D-414.md
