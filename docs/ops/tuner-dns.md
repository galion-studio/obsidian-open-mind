<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Why `tuner.galion.app` does not resolve, and the fix

Slot 118 `vps-tuner-dns`, written 2026-10-21 02:40 CEST (+02:00). READ-ONLY: nothing
was changed on the VPS, in the tunnel config or in DNS. Evidence:
[docs/real-tests/vps-tuner-dns.md](../real-tests/vps-tuner-dns.md).

## Short answer

The tunnel side is finished and live; **only the DNS record is missing.**

| Layer | State (2026-10-21 02:35 +02:00) | OK? |
|---|---|---|
| Origin `tuner` on 127.0.0.1:8650 | `tuner.service` running, `GET /health` → 200 | yes |
| Named tunnel `cloudflared.service` | running since 2026-10-18 22:36:14 +02:00, 4 HA connections, `/ready` 200 | yes |
| Ingress rule `tuner.galion.app → http://127.0.0.1:8650` | in config.yml (rule just above the 404 catch-all) **and** in the running config (`/config` on the metrics port lists `tuner.galion.app`) | yes |
| Tunnel type | locally managed (`cloudflared_orchestration_config_version 0`, config from the file) | — |
| DNS `tuner.galion.app` | **NXDOMAIN** at Cloudflare (1.1.1.1 DoH) and Google (8.8.8.8 DoH); no A, AAAA, CNAME or TXT; zone has no wildcard (`nonexistent-xyz.galion.app` is also NXDOMAIN) | **no** |
| Cloudflare edge, forced to the galion.app anycast IP | HTTPS: TLS handshake refused (no proxied record → no edge cert for the name); HTTP: 301 to HTTPS | consequence |
| Zone | `galion.app` on Cloudflare (NS elaine/peyton.ns.cloudflare.com), not DNSSEC-signed (no DS) | — |

## Root cause

1. On 2026-10-18 the tuner went live on 127.0.0.1:8650 and was published through an
   anonymous quick tunnel (`tuner-quick`, transient unit, random `*.trycloudflare.com`
   URL) as a stop-gap.
2. At ~22:36 +02:00 the same evening an ingress rule for `tuner.galion.app` was added
   to the **locally managed** tunnel's config.yml and `cloudflared` was restarted
   (config mtime and unit start time match to the minute).
3. For a locally managed tunnel, an ingress rule does **not** create DNS. The hostname
   also needs a proxied `CNAME tuner → <tunnel-id>.cfargotunnel.com` in the zone. That
   step was handed to Slavik ("Pending DNS … Slavik adds",
   `docs/session-2026-10-18-evening.md`) and has not been done.
4. It could not have been done from the VPS: `cloudflared tunnel route dns` needs the
   account origin certificate (`cert.pem`), and there is none in `/root/.cloudflared/`
   or `/etc/cloudflared/` (only the per-tunnel credentials file, which cannot edit DNS).

So the hostname has no record, resolvers get NXDOMAIN from Cloudflare's own
authoritative servers, and the (correct) ingress rule never sees a request.

## Fix proposal (owner decision D118-1 — a DNS change, needs Slavik)

**Option A (recommended): one record in the Cloudflare dashboard.**

1. dash.cloudflare.com → zone `galion.app` → DNS → Records → Add record:
   - Type `CNAME`, Name `tuner`,
   - Target `b2c049f7-7d08-4ec9-a7bc-eecfa76484a6.cfargotunnel.com` (the named
     tunnel ID from config.yml line 1; same value as in the 2026-10-18 session note),
   - Proxy status **Proxied** (orange cloud), TTL Auto.
   (Equivalent: Zero Trust → Networks → Tunnels shows this tunnel as locally
   configured; adding the record in DNS is the supported path for it.)
2. No VPS change is needed: the running `cloudflared` already routes the name.
   Nothing restarts.
3. Verify (read-only, ~1 min after saving):
   - `curl -s -H 'accept: application/dns-json' 'https://cloudflare-dns.com/dns-query?name=tuner.galion.app&type=A'`
     → `Status 0` with Cloudflare anycast answers (same pair as `one.galion.app`).
   - `curl -s https://tuner.galion.app/health` → `200 {"ok":true}`;
     `curl -sI https://tuner.galion.app/` → 200 with the tuner's security headers.
   - `journalctl -u cloudflared` shows no `ingressRule=` error for 8650.
4. Rollback: delete the record. Nothing else was touched.

**Option B: `cloudflared tunnel route dns b2c049f7-… tuner.galion.app`** on the VPS.
Needs `cloudflared tunnel login` first, which puts an account-wide origin cert on the
production box. More moving parts and a new long-lived credential on the VPS; not
recommended.

**Option C: scoped API token (Zone:DNS:Edit for galion.app) for automation.** Useful
if agents should manage DNS later, but it is a new credential and a standing
permission; separate decision, not needed for one record.

**Option D: do nothing.** Tuner stays reachable only via the quick-tunnel URL, which
changes on restart and disappears on reboot (transient unit).

## Follow-ups after the record works (separate approvals)

- **D118-2: retire `tuner-quick`.** `systemctl stop tuner-quick` (transient unit, no
  file to delete). Then remove the `tuner-quick` entry from galion-nerve's AETHER list
  so it does not report a missing component. It is an anonymous public tunnel with no
  access policy, so stopping it closes a second, unreviewed entry point.
- **Cosmetic, with the next config edit:** the `# Catch-all (required)` comment sits
  above the tuner rule, not above `- service: http_status:404`; move it. Also note
  config.yml is world-writable (`-rw-rw-rw-`), already raised in
  `docs/ops/tunnel-audit.md` (swarm/tunnel-audit-2) as `chmod 644`.
- **Optional:** public static tool, so no Cloudflare Access policy is needed; if Slavik
  wants it private, add an Access application for `tuner.galion.app` instead.

## What this slot did not do

No DNS change, no config edit, no restart, no stop of `tuner-quick`, no Cloudflare
login. The credentials file was checked for existence only, never read.
