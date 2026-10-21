<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover plan: Hermes device enrollment + omnode tunnel (live hermes-rs)

- **Approved: No.** Prepared only. Nothing was copied to the VPS; no unit, drop-in,
  cloudflared rule or DNS record was created or changed.
- Prepared: 2026-10-21 17:40 +02:00, branch `swarm/omnode-enroll` (fd63117 code).
  Real test: `docs/real-tests/omnode-enroll.md` (55/0 on the box).
- One service per session. If done on or after 2026-10-25 03:00, times are CET (+01:00).

## 1. What runs today (read-only, 2026-10-21)

| Item | Value | Source |
|---|---|---|
| Unit | `hermes-rs.service`, active | read-only probe 17:22:18 +02:00 |
| Binary | `/opt/galion/rust/bin/hermes-gateway-aa77864`, sha256 `eb691b29…` (no enroll code) via drop-in `50-rust-bin.conf`; nerve URL via `60-nerve-url.conf` | `config/ops/flight/hermes-rs.conf` (vps-preflight-scripts) |
| Unit backup | `/var/backups/hermes-rs.service-20261021-094621` | same |
| Listener | 127.0.0.1:8642 | probe |
| HTTP | `/health` 200; `/openmind/enroll` 401; `/openmind/node/tunnel` 401 | probe |
| Public route | **none**: no cloudflared ingress rule points to :8642 | probe (grep of cloudflared YAML) |

## 2. What would change

### 2.1 Binary

A new `hermes-gateway` built from the approved commit **on aarch64** (the VPS is aarch64;
the box has no SQLite C cross compiler, so a default-feature aarch64 build must be made
on an aarch64 host or with the sqlite-aarch64-cc toolchain). Install it **beside** the old one:
`/opt/galion/rust/bin/hermes-gateway-<sha>`; keep `hermes-gateway-aa77864` for rollback.
Point `50-rust-bin.conf` `ExecStart=` at the new file. Size: see D4 (x86_64 build is over
the 2 MiB budget; the aarch64 default build must be measured before deploy).

### 2.2 Unit environment (new drop-in `70-enroll.conf`)

```ini
[Service]
Environment=HERMES_RS_ENROLL=1
Environment=HERMES_RS_ENROLL_URL=https://<approved-hostname>/openmind/enroll
Environment=HERMES_RS_ENROLL_PENDING_TTL=900
Environment=HERMES_RS_ENROLL_TUNNELS=8
Environment=HERMES_RS_ENROLL_TUNNEL_PING=15
Environment=HERMES_RS_ENROLL_TUNNEL_LIFE=3600
# HERMES_RS_ENROLL_ALLOW_PUBLIC is NOT set: Hermes stays on 127.0.0.1, cloudflared reaches it on loopback.
# HERMES_RS_ENROLL_STORE default: $HERMES_HOME/rs-enroll.json (created mode 600).
```

If the unit has `ProtectSystem=strict`, `$HERMES_HOME` must already be in `ReadWritePaths`
(it is, for state.db) — check with `systemctl cat` at preflight, without printing env values.

### 2.3 Route through the existing Cloudflare tunnel

A new ingress rule in the cloudflared config, **above** the catch-all, for one hostname
Slavik approves (D3), restricted to the device routes only:

```yaml
- hostname: <approved-hostname>
  path: ^/openmind/(enroll(/status|/claim)?|node/tunnel(/answer)?)$
  service: http://127.0.0.1:8642
```

plus the DNS record (`cloudflared tunnel route dns <tunnel> <approved-hostname>`). Owner routes
(`/openmind/enroll/pending|approve|reject|ping|revoke|devices`) are **not** exposed; the owner
uses `hermes-gateway enroll …` on the VPS (loopback). Notes:
* The tunnel stream is `text/event-stream`; cloudflared and the Cloudflare edge stream it
  without buffering. Hermes pings every 15 s, well inside Cloudflare's 100 s idle limit;
  sessions end after 3600 s and the device reconnects.
* All public requests reach Hermes from 127.0.0.1; the enroll rate limits are global
  (20/min enroll+claim, 240/min status), so that is fine.
* Restarting cloudflared briefly drops every public hostname (one, console, obsidian).

### 2.4 Owner use

`API_SERVER_KEY` must be in the owner's shell for `hermes-gateway enroll pending|approve|reject`
(D6). The key is read from the environment and never printed.

## 3. Steps (when approved)

1. Preflight (read-only): unit text, drop-ins, binary sha, `/health` 200, cloudflared ingress
   list, free disk. Record in `docs/real-tests/`.
2. Copy the new binary, check sha256, `chmod 755`.
3. Write `70-enroll.conf`, edit `50-rust-bin.conf`, `systemctl daemon-reload`,
   `systemctl restart hermes-rs.service`. Check `/health` 200 and loopback
   `POST /openmind/enroll` with `{}` answers 4xx but not 404 (route mounted).
4. Add the ingress rule + DNS, restart cloudflared; check one/console/obsidian unchanged.
5. From the box: `omnode pair --dry-run https://<host>/openmind/enroll`, then a real pair of a
   throw-away laptop key; owner approves; `enroll devices` shows tunnel verified; `revoke` it.
6. Phone: Termux steps in `docs/omnode-enroll.md` §4.

## 4. Rollback

1. Remove the ingress rule (and DNS record), restart cloudflared.
2. `rm 70-enroll.conf`; point `50-rust-bin.conf` back to `hermes-gateway-aa77864`;
   `systemctl daemon-reload && systemctl restart hermes-rs.service`; `/health` 200.
3. If the unit text differs from the backup, restore `/var/backups/hermes-rs.service-20261021-094621`.
4. `rs-enroll.json` is inert without `HERMES_RS_ENROLL=1`; move it aside (keep, mode 600)
   rather than delete. Devices see 404/connection errors and keep retrying with backoff
   (max 60 s); they hold no server state that needs cleaning.

Time to roll back: under a minute; nothing else depends on the enroll routes.
