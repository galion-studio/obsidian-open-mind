<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: tuner-quick-unit (wave 4 slot 120)

Times Europe/Zagreb, CEST (+02:00), 2026-10-21. These are deploy/ and doc files only:
no Rust code changed, so nothing was built. **Nothing was installed, started, stopped
or written on the VPS.**

## 1. Offline unit checks on the box (04:04:34 +02:00, systemd 257.13 on the box)

The fake root `/tmp/tq` holds the box's `/usr/lib/systemd/system`, the staged unit in
`/etc/systemd/system/`, and `/bin/true` as `/usr/local/bin/cloudflared`.

```
$ systemd-analyze verify --man=no --root=/tmp/tq /etc/systemd/system/tuner-quick.service
verify_exit=0
$ systemd-analyze security --offline=true --root=/tmp/tq tuner-quick.service
→ Overall exposure level for tuner-quick.service: 1.4 OK :-)
```
The remaining items are PrivateNetwork, the INET/UNIX/NETLINK families, @resources,
PrivateUsers, IPAddressDeny, RootDirectory and char-rtc. A tunnel needs the network, and
@resources stays in the list because Go calls setrlimit at start. The live transient unit
scores **9.6 UNSAFE** on the VPS (systemd 255.4).

Before the root tree was added, verify without `--root` failed on one point only:
`/usr/local/bin/cloudflared is not executable` (the box doesn't have cloudflared).

Not tested: actually running cloudflared under this sandbox. The box has no systemd as
PID 1 (it runs tini), and a quick tunnel from the box would publish a public URL. The
runbook's preflight `systemd-run --wait … cloudflared --version` on the VPS covers the
first part. It needs approval.

## 2. Read-only VPS probes (04:03:16–04:05:00 +02:00)

Commands: `systemctl cat/show tuner-quick tuner cloudflared`, `readlink /proc/<pid>/exe`,
`/proc/<pid>/cmdline`, `cloudflared --version`, `stat`/`sha256sum` of the binary,
`ls -A /root/.cloudflared` (names only, the uuid masked, nothing read),
`systemd-analyze security tuner-quick` (it reads the loaded unit only), `ss -ltnp`,
journal grep for ERR/WRN (URLs and IPs filtered out), and GET `:20242/ready`, `:8670/state`
and `:8660/status.json`. The results are in `docs/ops/tuner-quick-unit.md` §1. The
trycloudflare URL and the connector id were not recorded.
