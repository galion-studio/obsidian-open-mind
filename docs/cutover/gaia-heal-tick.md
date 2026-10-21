<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover: Gaia heal-tick unit (`gaia-heal-tick@.service` + `.timer`)

- Written: 2026-10-21 13:45 CEST (+02:00), wave-4 slot 188, branch `swarm/gaia-heal-tick-unit` (base `swarm/gaia-loop-decisions` b40208a).
- Format follows docs/cutover-approval.md: what is approved, each item, open boxes, what was not done.
- This note does not install, enable, start, stop or replace anything. Nothing in it has been run on the VPS.

## Approved

**No.** Owner decision D-L1 (accepted 2026-10-21 08:15:59 CEST): no Gaia heal unit or timer on the VPS yet. Revisit when the cortex is live. At 13:23 CEST the VPS nerve was live (openmind-nerve aa77864, cycle 441, mode `live`), the cortex had no endpoint, and there was no `gaia*` unit, timer or binary.

| Item | Approved by Slavik |
| --- | --- |
| Create system user `gaia` (sysusers) | no |
| Copy `gaia` and `gaia-dummy` to /opt/galion/rust/bin | no |
| Write /etc/galion/gaia/heal-gaia-canary.env, /etc/galion/hades/gaia-gate.toml, admin.pub | no |
| Install the unit and timer in /etc/systemd/system | no |
| Mint the first token batch (admin side) | no |
| `systemctl enable --now gaia-heal-tick@gaia-canary.timer` | no |

## What gets installed (staged text in the repo)

| repo file | VPS path | owner, mode |
| --- | --- | --- |
| deploy/staged/gaia-heal-tick@.service | /etc/systemd/system/gaia-heal-tick@.service | root, 0644 |
| deploy/staged/gaia-heal-tick@.timer | /etc/systemd/system/gaia-heal-tick@.timer | root, 0644 |
| deploy/staged/gaia/sysusers-gaia.conf | /etc/sysusers.d/gaia.conf | root, 0644 |
| deploy/staged/gaia/heal-gaia-canary.env | /etc/galion/gaia/heal-gaia-canary.env | root:gaia, 0640 |
| deploy/staged/gaia/gaia-gate.toml | /etc/galion/hades/gaia-gate.toml | root:gaia, 0640 |
| admin public key (admin side; never the seed) | /etc/galion/hades/admin.pub | root, 0644 |
| `gaia`, `gaia-dummy` (x86_64 musl, built from the merged commit) | /opt/galion/rust/bin/gaia-&lt;sha12&gt; plus symlinks `gaia`, `gaia-dummy` | root, 0755 |

Writable state is only systemd's `StateDirectory=` (0700, owner `gaia`):

| path | what |
| --- | --- |
| /var/lib/gaia/hades/ledger.jsonl | hades-gate replay ledger (each token's nonce, consumed once) |
| /var/lib/gaia/hades/gaia-restarts.jsonl (+ `.lock`) | Gaia's global restart ledger (3/h cap; a new `--state` cannot reset it) |
| /var/lib/gaia/&lt;i&gt;/state/ | per-instance ledger `restarts.jsonl` and the healed service's log `&lt;i&gt;.log` |
| /var/lib/gaia/&lt;i&gt;/tokens/ | pre-minted `*.tok` (0600); used ones become `.tok.spent` / `.tok.refused` / `.tok.expired` |
| /var/lib/gaia/&lt;i&gt;/stop | pause switch (see below) |

The box real test checked this list. Every file the loop wrote was under `/var/lib/gaia`, and `/etc/galion` was byte-for-byte unchanged (docs/real-tests/gaia-heal-tick-unit.md).

## Design (and why)

1. **One long-running loop per target, re-armed by a timer.** `Type=exec` runs `gaia heal-tick --loop`. It ticks every 30 s plus 0-5 s of jitter, on the same 30 s cycle as the nerve. It stops on its own after 1 h (`--max-runtime-s 3600`), after 3 restarts in the run, after 5 errors in a row (exit 7), or when the stop file appears. `Restart=no`: the timer starts it again 5 min after it stops (`OnUnitInactiveSec=5min`, plus `OnBootSec=2min` and `OnActiveSec=30s`). A broken setup therefore costs one short run every 5 min, not a hot restart loop. `StartLimitBurst=6` per hour is a second fence.
   Alternative (D-U1): a oneshot per 30 s timer tick (`--loop --max-ticks 1`). It was rejected. It loses the loop's back-off and error count, starts a process every 30 s, and systemd would kill the restarted service at the end of every oneshot run unless `KillMode=process` (see 2).
2. **`KillMode=process`.** Gaia starts a healed service detached, in its own process group, but that service stays in the gaia unit's cgroup. With the default `KillMode=control-group`, `systemctl stop` of Gaia (or the hourly end of the loop) would kill every service it healed. With `process`, only the gaia main pid gets SIGTERM. The box test measured this: SIGTERM to the loop, the loop gone in 20 ms, the canary still answering with the same pid. Costs (D-U2): systemd logs "left-over process" on the next start; the healed service counts against the unit's `MemoryMax=128M` / `TasksMax=64` and inherits its sandbox; stopping the healed service is an operator act (Gaia never kills, D-L4).
3. **Stop file = pause.** The same path, `/var/lib/gaia/%i/stop`, is used twice. It is the loop's `--stop-file`: a running loop exits within one 100 ms sleep slice (measured 20-100 ms over 6 runs). It is also the unit's `ConditionPathExists=!`, so while it exists every timer start is skipped. Resume is `rm`.
4. **D-L2 nerve cap.** `--nerve 127.0.0.1:8670` is the live nerve. Before a token is used, and again under the budget lock, every tick that could restart reads `GET /state` and counts `max(restarts_1h, reflex live_1h)` for the service. Gaia plus the nerve stay at or under 3 per rolling hour. A broken `/state` fails closed. Box test: nerve 2 + Gaia 1 = capped with no token used; nerve 0 = healed. `/state` 500 = exit 7 after 5 errors, 0 tokens. The live VPS `/state` (13:31:57 CEST, cycle 457) parses: `galion-one`, `cloudflared` and `hermes-rs` at `restarts_1h` 0, so 3 left. `gaia-canary` is not watched by the nerve, so it counts 0.
5. **D-L3 tokens only from the token dir.** There is no `--token-file` and no standing grant. Batches of 1-3, at most 3 live, TTL ≤ 1 h. hades-gate's kill switch is `/etc/galion/hades/gaia.DISABLED` (root-owned, read-only to Gaia): `touch` refuses every heal without needing a token.
6. **Scope.** Gaia's apply path refuses anything outside 127.0.0.1:19000-19999, and systemctl or other service managers (checked by the static test with 8642, 8600-8700 and `systemctl restart hermes-rs`). So the only possible instance today is a test canary: `gaia-dummy --port 19880` (port free on the VPS at 13:27 CEST; the only 19xxx listener was 127.0.0.1:19120). The unit is the vehicle for the day the scope is widened, which is a separate decision.
7. **Hardening.** User `gaia` (no shell), no capabilities, `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, kernel, clock, hostname and cgroup protection, `ProtectProc=invisible`, `RestrictNamespaces`, `MemoryDenyWriteExecute`, `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`, `IPAddressDeny=any` + `IPAddressAllow=localhost`, `SystemCallFilter=@system-service ~@privileged @resources`, `UMask=0077`, `ReadOnlyPaths=/etc/galion`, `Nice=10`, `CPUQuota=20%`. `systemd-analyze security --offline` exposure: **1.1 OK** (box, systemd 257). `ProcSubset=pid` is not set: the D-L4 hung check reads `/proc/net/tcp{,6}`. No `PrivateNetwork`: the nerve is on the host loopback.
   VPS (LXC, systemd 255): every directive used here is already used by an active VPS unit. Read-only grep at 13:28 CEST: PrivateDevices (galion-nerve, openmind-memory), ProtectProc=invisible (systemd-networkd), IPAddressDeny (journald, logind), SystemCallFilter (rsyslog), MemoryDenyWriteExecute (logind), StateDirectory (galion-nerve, hermes-rs). **Caveat (D-U5):** `systemctl show systemd-logind -p IPEgressBytes` = `[no data]`, so IP accounting/BPF firewalling probably does not work in this LXC, and `IPAddressDeny` may be silently unenforced. Gaia itself only dials 127.0.0.1, so this is defence in depth only.

## Preconditions (all open)

1. D-L1 revisit: the cortex is live (`/api/v1/cortex` answers on the VPS).
2. D-L2b decided: the per-unit nerve limit (`min(3, reflex_restarts_per_hour)`?). This does not affect the canary.
3. Token custody decided: who runs `gaia tokens mint` with the admin seed, on which machine, and how the `.tok` files reach `/var/lib/gaia/<i>/tokens`.
4. `gaia` / `gaia-dummy` rebuilt from the merged main commit, sha256 recorded, and the box real test re-run with that build.

## Steps (for Slavik; nothing below has been run)

```
# 0. preflight (read-only)
ssh slavik-vps 'systemctl list-units --all "gaia*"; ss -ltn | grep -c ":19880 " ; curl -s -m5 127.0.0.1:8670/state | head -c 200'
# 1. copy + verify (artefacts from the merged commit)
scp gaia gaia-dummy SHA256SUMS slavik-vps:/var/tmp/gaia-cutover/
ssh slavik-vps 'cd /var/tmp/gaia-cutover && sha256sum -c SHA256SUMS && install -m0755 gaia /opt/galion/rust/bin/gaia-<sha12> && install -m0755 gaia-dummy /opt/galion/rust/bin/gaia-dummy-<sha12> && ln -sfn gaia-<sha12> /opt/galion/rust/bin/gaia && ln -sfn gaia-dummy-<sha12> /opt/galion/rust/bin/gaia-dummy'
# 2. user, config, units
install -m0644 sysusers-gaia.conf /etc/sysusers.d/gaia.conf && systemd-sysusers /etc/sysusers.d/gaia.conf
install -d -m0750 -o root -g gaia /etc/galion/gaia /etc/galion/hades
install -m0640 -o root -g gaia heal-gaia-canary.env /etc/galion/gaia/ ; install -m0640 -o root -g gaia gaia-gate.toml /etc/galion/hades/
install -m0644 admin.pub /etc/galion/hades/admin.pub
install -m0644 'gaia-heal-tick@.service' 'gaia-heal-tick@.timer' /etc/systemd/system/ && systemctl daemon-reload
systemd-analyze verify gaia-heal-tick@gaia-canary.service gaia-heal-tick@gaia-canary.timer
systemd-analyze security gaia-heal-tick@gaia-canary.service | tail -1
# 3. state dirs (same as StateDirectory=) + first batch
install -d -m0700 -o gaia -g gaia /var/lib/gaia /var/lib/gaia/hades /var/lib/gaia/gaia-canary /var/lib/gaia/gaia-canary/state /var/lib/gaia/gaia-canary/tokens
runuser -u gaia -- /opt/galion/rust/bin/gaia tokens request --token-dir /var/lib/gaia/gaia-canary/tokens --count 1 --ttl-s 900 \
  --service gaia-canary --port 19880 --allow-ports 19880-19880 -- /opt/galion/rust/bin/gaia-dummy --port 19880
#   plan sha256 must be bc4dc98038f6609f6f8e33a82ff3da6cb101fc7636de929780c4fc9fa2e4e20f (tests/staged_heal_unit.rs)
#   ADMIN SIDE: gaia tokens mint --request req-....request.json --seed <admin seed>  -> copy *.tok back, chown gaia, chmod 0600
# 4. start
systemctl enable --now gaia-heal-tick@gaia-canary.timer
```

**Verify (first 10 min):** `systemctl status gaia-heal-tick@gaia-canary.service` is active. `journalctl -u gaia-heal-tick@gaia-canary -o cat` shows one JSON line per tick: the first tick is `restarted` with token `.spent`, then `healthy`. `curl 127.0.0.1:19880/health` returns `status ok`. The nerve has `gaia.loop.started`, `gaia.heal.applied`, then `service.probe` every ~30 s. `gaia tokens status --token-dir /var/lib/gaia/gaia-canary/tokens` shows no token printed. Live services are unchanged: `galion-nerve`, `hermes-rs`, `openmind-memory` and `galion-one` keep the same `NRestarts` and MainPID.

## Pause, kill switch, rollback

- **Pause:** `touch /var/lib/gaia/gaia-canary/stop`. The loop exits within about 0.1 s, and the timer skips every start until `rm`.
- **Kill switch (heals only):** `touch /etc/galion/hades/gaia.DISABLED`. The loop keeps probing and reporting, and every restart is refused.
- **Rollback (full):**
```
touch /var/lib/gaia/gaia-canary/stop
systemctl disable --now gaia-heal-tick@gaia-canary.timer
systemctl stop gaia-heal-tick@gaia-canary.service
# KillMode=process: the canary keeps running. Stop it as operator (its pid is in the ledger / ss -ltnp :19880):
kill <gaia-dummy pid>
tar -C /var/lib -czf /var/backups/gaia-state-$(date +%Y%m%d-%H%M).tgz gaia
rm /etc/systemd/system/gaia-heal-tick@.service /etc/systemd/system/gaia-heal-tick@.timer && systemctl daemon-reload && systemctl reset-failed 'gaia-heal-tick@*'
rm -r /etc/galion/gaia /etc/galion/hades/gaia-gate.toml /etc/galion/hades/admin.pub /var/lib/gaia
rm /opt/galion/rust/bin/gaia /opt/galion/rust/bin/gaia-dummy /opt/galion/rust/bin/gaia-<sha12> /opt/galion/rust/bin/gaia-dummy-<sha12>
# optional: userdel gaia; rm /etc/sysusers.d/gaia.conf
```
No live unit, port or file outside the paths above is touched by install or rollback. /opt/galion/openmind is not touched.

## Owner decisions (open; not decided here)

| # | Question | Recommendation |
| --- | --- | --- |
| D-U1 | Unit shape: long-running loop + 5-min re-arm timer, or a oneshot per 30 s tick | Loop + re-arm timer (keeps back-off, jitter, one process) |
| D-U2 | `KillMode=process` (healed services outlive Gaia) or `control-group` (stopping Gaia stops what it healed) | `process`; a later step could heal through each service's own unit, which needs its own decision because systemctl is forbidden on the apply path today |
| D-U3 | Static user `gaia` (sysusers) or `DynamicUser=yes` | Static: tokens are copied in by root and must keep a stable owner |
| D-U4 | First instance: the `gaia-dummy` canary on 19880, or nothing until the scope is widened | Canary only, after D-L1 says yes; it proves tokens, cap, nerve events and pause on the real host with zero live risk |
| D-U5 | `IPAddressDeny` is probably not enforced in this LXC (no IP accounting data) | Keep it as harmless defence in depth and say so; do not count on it |

Still open from slot 175: D-L1 revisit trigger, D-L2b per-unit caps, token custody.

## Not done

Nothing was installed, copied or enabled on the VPS. No `systemctl` write was used. The VPS was only read (`systemctl` show/list/is-active, `ss -ltn`, `grep` of unit files, GET `/state`). No secret was read or written. No box in this file is checked.
