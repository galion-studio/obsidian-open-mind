<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Runbook step: make tuner-quick a real unit (staged, not installed)

Branch `swarm/tuner-quick-unit` (wave 4 slot 120), on `swarm/cutover-prep` a5ac49b.
**Nothing here has been run on the VPS.** The file is `deploy/staged/tuner-quick.service`.
Every command in section 3 changes the VPS, and the public URL changes with most of them,
so each one needs Slavik's approval first. Times are Europe/Zagreb (CEST, +02:00).

## 1. What runs today (read-only probe, 2026-10-21 04:03:16–04:05:00 +02:00)

| | live |
|---|---|
| unit | `tuner-quick.service`, **transient** (`/run/systemd/transient/tuner-quick.service`, 291 B, sha256 `626fa1c6022b…`), made with `systemd-run` on 2026-10-18 at 19:19:30 +02:00. The unit is lost on a reboot, and so is the URL |
| definition | `Description=Temporary trycloudflare tunnel for tuner`, `Restart=on-failure` (100 ms), `ExecStart=/usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650`. No `[Install]`, no `User=`, no sandbox |
| process | PID 1198838, NRestarts 0, RSS about 19 MB (systemd MemoryCurrent 19,087,360 B; nerve reports 32,948 kB RSS) |
| binary | `/usr/local/bin/cloudflared` → `/workspace/tooling/bin/cloudflared` → `/opt/galion/tooling/bin/cloudflared` (`/workspace` → `/opt/galion` on the VPS). 39,838,488 B, sha256 `03f1f25d1cc93b9a…`, version 2026.9.1. Its journal says 2026.10.2 is out. The path is 0775 root:root (group-writable) |
| runs as | **root**, with `systemd-analyze security` exposure **9.6 UNSAFE**. Root's home holds `/root/.cloudflared/<uuid>.json`, the credential of the separate **named** tunnel (`cloudflared.service`). The quick tunnel doesn't need that file, but it can read it |
| listeners | metrics on `127.0.0.1:20242`. `GET /ready` → 200, `readyConnections: 1`. The named tunnel's metrics are on 20241 |
| watchers | galion-nerve `/state`: `tuner-quick` healthy, note "never restarted: its trycloudflare URL would change", `reflex_restarts_per_hour 0`. galion-health `/status.json` lists `tuner-quick.service` active/running. Both find it **by unit name** |
| journal | 48 lines in total since 2026-10-18. 1 tunnel registration, 1 DNS-resolver timeout (2026-10-20 13:50:11 +02:00, recovered), daily "version outdated" warnings, and ICMP-proxy disabled (GID 0 not in `ping_group_range`) |

## 2. The staged unit: what changes and why

| | transient (live) | `deploy/staged/tuner-quick.service` |
|---|---|---|
| survives reboot | no | yes: `[Install] WantedBy=multi-user.target` |
| program + args | `cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650` | the same + `--metrics 127.0.0.1:20242` (pinned to the port it already uses; otherwise cloudflared picks the first free one of 20241–20245) |
| unit name | `tuner-quick.service` | **the same**, so nerve and galion-health need no change |
| restart | on-failure, 100 ms, no storm limit | always, 10 s; at most 5 starts per 10 min. cloudflared can exit 0 after it loses the edge |
| tie to tuner | none | `After=` + `Wants=tuner.service`, **not** `Requires=`. galion-deploy restarts tuner in the cutover, and `Requires=` would restart the tunnel too, which means a new URL |
| user | root | `DynamicUser=yes`, `ProtectHome=yes`, so `/root/.cloudflared` can't be read |
| sandbox | none (9.6 UNSAFE) | strict FS, no capabilities, `@system-service` syscalls minus `@privileged` (EPERM, not SIGSYS), INET/INET6/UNIX/NETLINK only, MemoryMax 256M, TasksMax 64 → **1.4 OK** offline |

Compared with `deploy/staged/tuner-publish.service` (the O4 alternative): that unit needs the
`tuner-publish` binary from ws/one-desk-tuner (not in main), writes the URL to
`/run/tuner/quick-url.json`, runs as root, and has `Requires=tuner.service`. That last
setting rotates the URL on every tuner deploy, so it should become `Wants=` if that unit
is ever chosen. The unit here needs nothing new: only the cloudflared that is already installed.

## 3. Install step (needs approval; pick ONE path under O4)

Copy first and check it. These are reads and a write to /var/tmp only:
```
scp deploy/staged/tuner-quick.service slavik-vps:/var/tmp/galion-cutover/
ssh slavik-vps 'systemd-analyze verify /var/tmp/galion-cutover/tuner-quick.service && echo OK'
ssh slavik-vps 'systemd-analyze security --offline=true /var/tmp/galion-cutover/tuner-quick.service | tail -1'
# optional sandbox smoke test, no tunnel created:
ssh slavik-vps 'systemd-run --wait --pipe -p DynamicUser=yes -p ProtectHome=yes -p ProtectSystem=strict /usr/local/bin/cloudflared --version'
```

**Path A — switch now (the public URL CHANGES once).** About 5–10 s with no public tuner.
```
journalctl -u tuner-quick -o cat | grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' | tail -1   # old URL, for the record
systemctl stop tuner-quick                       # transient file is removed when the unit unloads
systemctl reset-failed tuner-quick 2>/dev/null; test ! -e /run/systemd/transient/tuner-quick.service && echo transient-gone
install -m 0644 -o root -g root /var/tmp/galion-cutover/tuner-quick.service /etc/systemd/system/tuner-quick.service
systemctl daemon-reload && systemctl enable --now tuner-quick
```

**Path B — switch at the next reboot (URL unchanged until then).** A transient unit in
`/run/systemd/transient` comes before `/etc/systemd/system` in systemd's search path. The
live tunnel keeps running from it, and after a reboot only the `/etc` file is left.
`systemctl enable` may refuse while the transient copy is loaded, so B creates the same
symlink `enable` would:
```
install -m 0644 -o root -g root /var/tmp/galion-cutover/tuner-quick.service /etc/systemd/system/tuner-quick.service
ln -s ../tuner-quick.service /etc/systemd/system/multi-user.target.wants/tuner-quick.service
systemctl daemon-reload
systemctl show tuner-quick -p FragmentPath,Transient     # still /run/systemd/transient/..., yes (expected)
```
This one is untested: on the box PID 1 is tini, so the precedence could not be shown
live. If `daemon-reload` makes the `/etc` file win, the running process is not touched
(no restart), and `FragmentPath` just shows `/etc/…`.

**Path C — O4 = do not publish the tuner.** `systemctl stop tuner-quick`. The transient
unit goes away; install nothing. Then remove `tuner-quick` from the nerve/galion-health
watch lists, or they report it absent.

### Checks after A (GET only)
```
systemctl show tuner-quick -p FragmentPath,Transient,UnitFileState,ActiveState,User
#   /etc/systemd/system/tuner-quick.service, no, enabled, active
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:20242/ready          # 200
U=$(journalctl -u tuner-quick -o cat | grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' | tail -1)
curl -s -o /dev/null -w '%{http_code}\n' "$U/health"                           # 200 {"ok":true}
ps -o user= -p $(systemctl show -p MainPID --value tuner-quick)               # not root
curl -s http://127.0.0.1:8670/state | grep -o '"tuner-quick"[^}]*"status":"[a-z]*"'
```
Do not commit or post the URL. It is a public entry point (SEC-5).

### Rollback (the URL changes again)
If the sandbox breaks cloudflared, `journalctl -u tuner-quick` shows why. Add a drop-in
`/etc/systemd/system/tuner-quick.service.d/10-root.conf` with `[Service]` `DynamicUser=no` and
then `systemctl daemon-reload && systemctl restart tuner-quick`. To go back to exactly what runs today:
```
systemctl disable --now tuner-quick && rm /etc/systemd/system/tuner-quick.service && systemctl daemon-reload
systemd-run --unit=tuner-quick --description='Temporary trycloudflare tunnel for tuner' \
  -p Restart=on-failure /usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650
```

## 4. Where it sits in the cutover

`docs/ops/cutover-tuner-nerve.md` §9. It is independent of the tuner/nerve binary swaps and
should **not** run in the same window. If chosen, path B can go in early (the binary swaps
don't touch it). Path A belongs in a separate window after O4.

## 5. Owner decisions

- **O4 / SEC-5 (unchanged, still first):** keep a public trycloudflare URL for the tuner? A, B, or C above. Another option is a stable hostname through the named tunnel; slot 118 `swarm/vps-tuner-dns` has the DNS proposal.
- **O4a, if kept:** this unit (cloudflared only, sandboxed) or `tuner-publish.service` (needs a new binary, writes the URL to a file)?
- **O4b:** cloudflared 2026.9.1 is behind 2026.10.2. It sits in a group-writable dir (`/opt/galion/tooling/bin`, 0775) and runs as root today. Moving it to a root-only 0755 path is a separate change.
