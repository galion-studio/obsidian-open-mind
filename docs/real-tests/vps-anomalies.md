<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# Real tests: vps-anomalies (slot 168), read-only, 2026-10-21 08:26-08:33 +02:00

All probes GET / status reads. No VPS state changed. Written 2026-10-21 08:31 +0200.

## HTTP (curl -s -o /dev/null -w code)
- box -> https://galion.app/ : 502 (cf-ray ...-SEA); www.galion.app 502; one.galion.app 200; galion.studio 502; browser UA also 502.
- VPS -> https://galion.app/ : 403, cf-mitigated=challenge, body title "Just a moment..." (WAW); www 403 challenge; one.galion.app 403 challenge; browser UA 403; IPv4 403; IPv6 no route (000).
- VPS -> https://galion.studio/ : 502, no cf-mitigated.
- VPS loopback: 127.0.0.1:4000 and [::1]:4000 connection refused (000); ss: 0 listeners on :4000.
- cloudflared ingress: galion.app and www.galion.app -> http://localhost:4000.
- cloudflared journal: 40 x "[::1]:4000: connect: connection refused" in last 60 min; first since journal start at 2026-10-18 14:58:21 +02:00 (journal begins 14:57:35 +02:00).
- nerve 127.0.0.1:8670/state: 200 (cortex not deployed; nerve used).

## systemd
- systemctl --failed: run-u11093/4/5.service only (loaded failed failed).
- each: ExecStart=/usr/bin/systemctl is-active ssh.service, DynamicUser=yes (+ProtectSystem=strict+PrivateDevices / none / +PrivateDevices), start=exit 2026-10-18 22:13:59 +02:00, status 1.
- ssh.service active since 2026-10-12 07:01:36 +02:00, NRestarts=0.
- systemctl is-system-running: degraded.
- setpriv --reuid=65534 systemctl is-active ssh.service: active, exit 0 (static non-root works).
- dbus.service/socket active; libnss-systemd installed, nsswitch "files systemd"; systemd-detect-virt: lxc; uid_map 0 100000 65536.
- galion-health journal 22:13:52 +02:00 failing=[11 units]; restarts 22:14:06-22:14:37; commit e43d7ab 22:15:08 +02:00 sets User=root with dbus/DynamicUser comment.
