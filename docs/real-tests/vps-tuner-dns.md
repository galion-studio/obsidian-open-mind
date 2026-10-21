<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test evidence: vps-tuner-dns (slot 118)

All probes READ-ONLY. Box = swarm build box; VPS = production host (address omitted).
Secrets: none printed; credentials-file path redacted, file not read.

## Public resolvers (box, 2026-10-21 02:35:01 +02:00)

DoH JSON, `type=A` unless noted:

| Name | Cloudflare 1.1.1.1 | Google 8.8.8.8 |
|---|---|---|
| tuner.galion.app | Status 3 (NXDOMAIN), SOA elaine.ns.cloudflare.com serial 2418027788 | Status 3 (NXDOMAIN), same SOA |
| tuner.galion.app CNAME / AAAA / TXT | — | Status 3 each |
| galion.app | 0, 2 Cloudflare anycast A | 0, same |
| one.galion.app | 0, same pair (proxied tunnel host) | 0, same |
| console.galion.app, tulu.galion.app | 0, same pair | 0, same |
| nonexistent-xyz.galion.app (wildcard check) | Status 3 | Status 3 |
| galion.app NS | — | elaine.ns.cloudflare.com, peyton.ns.cloudflare.com |
| galion.app DS | — | none (zone not DNSSEC-signed) |
| `<tunnel-id>.cfargotunnel.com` AAAA | — | fd10:aec2:5dae:: (tunnel exists) |

Quad9 DoH (port 5053) timed out from the box; not needed.

## Cloudflare edge, name forced to the galion.app anycast IP (box, 02:35:55 +02:00)

```
tuner.galion.app  https: TLS alert "decode error", handshake refused (http 000)
tuner.galion.app  http:  301 (to https)
one.galion.app    https: 200, 206340 B, <title>One</title>
```

## VPS (read-only, 2026-10-21 02:35:30 +02:00)

```
cloudflared.service  active running  "Cloudflare tunnel"  (since 2026-10-18 22:36:14 +02:00)
tuner-quick.service  active running  "Temporary trycloudflare tunnel for tuner"
tuner.service        active running  "Ukulele tuner static server (127.0.0.1:8650)"
ExecStart: cloudflared tunnel --no-autoupdate --config <galion-infrastructure>/cloudflared/config.yml run
config.yml: -rw-rw-rw- root root 3319 B, mtime 2026-10-18 22:36 +02:00
  line 1   tunnel: b2c049f7-7d08-4ec9-a7bc-eecfa76484a6
  line 128 # Catch-all (required)
  line 129 - hostname: tuner.galion.app
  line 130   service: http://127.0.0.1:8650
  line 132 - service: http_status:404
credentials file: present (not read)
/root/.cloudflared/cert.pem: absent; /etc/cloudflared/cert.pem: absent
getent hosts tuner.galion.app: does not resolve
getent hosts one.galion.app: resolves (Cloudflare v6 anycast)
ss -ltn: 127.0.0.1:8650 tuner; 127.0.0.1:20241 cloudflared (named); 127.0.0.1:20242 cloudflared (quick)
GET 127.0.0.1:8650/health -> 200
GET 127.0.0.1:20241/ready -> {"status":200,"readyConnections":4,...}
GET 127.0.0.1:20241/config -> running ingress contains "tuner.galion.app"
metrics: cloudflared_tunnel_ha_connections 4
         cloudflared_orchestration_config_version 0   (locally managed)
         cloudflared_config_local_config_pushes 1, errors 0
cloudflared version 2026.9.1
journalctl -u cloudflared (last 2 h): errors only for dead origins 3011, 4001
  (merlinelement.com, galion.studio); none for 8650 (no traffic reaches the rule).
```

Conclusion: origin and tunnel ingress are correct and live; the only missing piece is
the proxied `CNAME tuner → <tunnel-id>.cfargotunnel.com` in the galion.app zone.
