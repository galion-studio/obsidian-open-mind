<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# VPS anomalies, 2026-10-21 (slot 168, read-only)

Investigated 2026-10-21 08:26-08:33 CEST (+02:00) from the swarm box over ssh, read-only.
Nothing on the VPS was changed: no reset-failed, no unit, DNS, Cloudflare or permission edits.
Raw probe outputs: `docs/real-tests/vps-anomalies.md`.

## 1. galion.app: 403 from the VPS vs 502 in the record

**Both numbers are right. They come from two different layers of Cloudflare.**

| Where the request starts | galion.app | www.galion.app | one.galion.app | galion.studio |
|---|---|---|---|---|
| Swarm box (egress via SEA edge) | **502** | 502 | 200 | 502 |
| VPS (OVH, WAW edge) | **403**, `cf-mitigated: challenge` | 403 challenge | 403 challenge | 502 (no challenge) |

- **403 = Cloudflare edge security challenge, not the origin.** From the VPS every galion.app
  hostname (even healthy one.galion.app) answers 403 with header `cf-mitigated: challenge` and
  the HTML `<title>Just a moment...</title>` (Cloudflare managed challenge). It shows up with
  both curl and a browser User-Agent. The request never reaches the tunnel. galion.studio,
  which is a different zone, is *not* challenged from the same IP and gives 502. So a
  **zone-level security setting on galion.app** challenges the VPS's data-centre IP. The
  likely cause is Bot Fight Mode, Security Level "High/I'm under attack", or a WAF/managed
  rule aimed at hosting ASNs such as OVH. Which of these it is can only be seen in the
  Cloudflare dashboard, and this slot does not touch Cloudflare.
- **502 = real origin failure.** From a non-challenged client the edge lets the request
  through. cloudflared then routes `galion.app` and `www.galion.app` to
  `http://localhost:4000` (config `/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`).
  **Nothing listens on :4000** (0 listeners; local GET 127.0.0.1:4000 and [::1]:4000 refused).
  cloudflared logged 40 `[::1]:4000: connect: connection refused` lines in the last 60 min.
  The first one is at 2026-10-18 14:58:21 +02:00, one minute after the oldest retained journal
  entry (14:57:35 +02:00), so the origin has been down for the whole retained journal.
- **Why the record disagreed:** the record's 502 came from probes off the VPS (box/public).
  Slot 142's 403 came from curl *on the VPS*, which the challenge layer answers first and hides
  the 502 behind it. Neither number is a VPS fault on its own.

**Consequence for monitoring:** any VPS-side public probe of a galion.app hostname
(galion-one's "5 curls via Cloudflare every 15 s" found in slot 116, link-watch, future cortex
edge checks) sees 403 for every hostname, healthy or not. It cannot tell up from down. Probes
of galion.app from the VPS must go to the loopback origin, or be exempted by a narrowly scoped
Cloudflare rule.

## 2. The 3 failed run-u* units (2026-10-18)

| Unit | Started / failed | Transient properties | ExecStart | Result |
|---|---|---|---|---|
| run-u11093.service | 2026-10-18 22:13:59 +02:00 | DynamicUser=yes, ProtectSystem=strict, PrivateDevices=yes | `/usr/bin/systemctl is-active ssh.service` | exit 1 |
| run-u11094.service | 22:13:59 +02:00 | DynamicUser=yes | same | exit 1 |
| run-u11095.service | 22:13:59 +02:00 | DynamicUser=yes, PrivateDevices=yes | same | exit 1 |

**What ran:** three `systemd-run` probes, all in the same second, each checking whether a
sandboxed **DynamicUser** process can run `systemctl is-active`. They form a bisection: full
sandbox, bare DynamicUser, DynamicUser plus PrivateDevices. Unit files are still in
`/run/systemd/transient/`. No stdout was captured in the journal.

**Who/why (from journal + git):** this happened during the ws/foundation deploy of
galion-health on the evening of 2026-10-18:
- 22:13:33 +02:00 galion-health (12 checks) started. At 22:13:52 +02:00 it reported **every** service failing
  (`failing=[openmind-memory,hermes-rs,link-watch-rs,galion-desk,tuner,tuner-quick,console,cloudflared,journald,ssh,galion-backup.timer]`),
  because its `systemctl is-active` calls failed inside its sandbox.
- 22:13:55-22:13:59 +02:00: root ssh sessions 48868/48870 (deploy agent, same key as the other
  deploy sessions) ran a daemon-reload and the 3 probes. All 3 failed, even bare DynamicUser.
- 22:14:06-22:14:37 +02:00: galion-health was restarted several times (it crash-looped briefly at
  22:14:17-22:14:34 +02:00) and then came up stable.
- Commit e43d7ab (22:15:08 +02:00, ws/foundation) ships galion-health.service with
  `User=root` and the comment *"Runs as root only so `systemctl is-active` can use
  /run/systemd/private (system dbus refuses DynamicUser here)"*. galion-nerve later copied the
  same reasoning.

**Why they failed:** the probes ran as a DynamicUser, which cannot use the private systemd
socket (root only). On this host (unprivileged LXC, uid map 0→100000 x 65536, systemd 255,
dbus 1.14.10), the system-bus path failed for DynamicUser at that moment. Checked today
read-only: a *static* non-root user (`setpriv --reuid=65534 systemctl is-active ssh.service`)
gets `active`, exit 0. So the problem is specific to DynamicUser, not "dbus refuses all
non-root". The exact reason dbus rejected the dynamic UID was not confirmed. Confirming it
would need new transient units, which is a write. ssh.service itself was fine throughout
(active since 2026-10-12 07:01:36 +02:00, NRestarts=0).

**Side effect still live:** these 3 leftover failed units are the **only** failed units. They
are why `systemctl is-system-running` says **`degraded`** right now. Any check that reads
system state (nerve/cortex/Gaia, a future alert) sees a permanent false "degraded" from a
3-day-old experiment. The units are harmless otherwise and disappear on reboot (/run is tmpfs).

## Recommendations (owner decisions for Slavik; none applied)

- **D-A1 (recommended): `systemctl reset-failed run-u11093.service run-u11094.service run-u11095.service`.**
  This clears the false `degraded`. It cannot affect a running service (the units are dead
  and transient). It is a live write, so Slavik approves it. Alternative: leave them until
  the next reboot.
- **D-A2: galion.app origin :4000.** Decide what should serve galion.app/www (start the site
  on :4000, or point the rule at a live origin, or remove the rule). This is the same family
  as slot 159's 21 closed ingress ports, and the tunnel-fix-502 executor approved 2026-10-21 08:23 +02:00 may
  already cover it. Coordinate there instead of acting twice.
- **D-A3: VPS → galion.app challenge.** Options: (a) keep it, and make every VPS-side check use
  loopback origins (recommended, no Cloudflare change); (b) add a Cloudflare WAF skip rule
  for the VPS address only (a Cloudflare change, needs Slavik plus dashboard access); (c) lower
  the zone setting (not recommended). Also check the challenge setting with the galion.app TLS
  renewal (cert expires 2026-10-31 18:28 CET).
- **D-A4: DynamicUser for galion-health/galion-nerve.** Optional follow-up: find out why dbus
  rejected DynamicUser (one throwaway transient probe, approved, followed by reset-failed), or
  switch to a static system user instead of root, since a static non-root uid works over dbus.
  That would drop root from two always-on services.
