<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# guard-shadow: `galion-guard` shadow mode, fail2ban comparison, rollout

Branch `swarm/guard-shadow` (base `swarm/guard-rust` d5f7874), wave-4 slot
123, ports 19930-19939 (only 19930 is used, as a stand-in jail port in the
real test; nothing listens on it). Written 2026-10-21 04:55 +02:00 (CEST).
Nothing was installed, started, stopped or written on the VPS; every VPS
command was a read-only probe (`journalctl` reads, `cat`/`grep` of
`/var/log/fail2ban.log`, `fail2ban-client status sshd`).

## 1. What it is

`galion-guard` (see docs/guard-rust.md) is the Rust replacement for the VPS
fail2ban sshd jail. Before it may touch the firewall it should run next to
fail2ban and prove, on live traffic, that it would make the same decisions.
That needs:

1. **Shadow mode**: the full daemon (journal source, filter, sessions, jail,
   state, control socket, CLI) with a backend that cannot write to nftables.
2. **A fail2ban-format log** of what it would have done.
3. **A comparison report** that lines the two logs up and says PARITY or
   lists every difference.

## 2. Shadow backend (`backend = "shadow"`)

| Property | How it is guaranteed |
|---|---|
| No nft writes, ever | `daemon::backend_for` maps `"shadow"` to `nft::Shadow`, an in-memory set with **no** `nft` path, no `Command`, no script log. There is no code path from shadow mode to a process spawn. `galion-guard teardown` in shadow mode returns before building any backend ("nothing to remove (nft not run)"). |
| Proven in the real test | The real-test config points `nft` at a path that does not exist; any attempt would log a firewall/spawn error. The daemon log has none, `backend_ready = true`, `status --json` reports `"shadow": {"firewall_writes": 0, ...}`. |
| Even a bug could not write | Staged systemd drop-in `deploy/staged/galion-guard-shadow.conf`: `CapabilityBoundingSet=CAP_DAC_READ_SEARCH`, `RestrictAddressFamilies=AF_UNIX` (no CAP_NET_ADMIN, no netlink) for the shadow phase. |
| Same decisions as production | Only the backend differs; the engine, jail, state and sources are the production code. Bans, increments, expiry, manual `ban`/`unban`, restarts and `status` behave as with `nft`. |
| Operator can see it | `status` prints `Mode: SHADOW (log-only, no firewall writes) -> <log>`; the start-up line says `SHADOW MODE`. |

Config (`[guard]`): `backend = "shadow"`, `shadow_log` (default
`shadow.log` next to the state file), `shadow_zone = "zagreb" | "utc"`.
`dry-run` (logs the nft scripts it would run) stays available.

## 3. Shadow log (fail2ban format)

Same layout as fail2ban 1.0.2 (`asctime name-24 [pid]: LEVEL-7 message`),
logger names `galion-guard.*` so nobody mistakes it for the real log:

```text
2026-10-21 04:39:55,000 galion-guard.filter     [3661530]: INFO    [sshd] Found 203.0.113.66 - 2026-10-21 04:39:54
2026-10-21 04:39:55,000 galion-guard.shadow     [3661530]: INFO    [sshd] Ignore <protected address withheld> by Allowlist
2026-10-21 04:39:55,000 galion-guard.actions    [3661530]: NOTICE  [sshd] Ban 203.0.113.66
2026-10-21 04:39:55,000 galion-guard.shadow     [3661530]: INFO    [sshd] would ban 203.0.113.66 until 2026-10-21T04:39:59+02:00 (3 failures; no firewall write)
2026-10-21 04:39:59,000 galion-guard.actions    [3661530]: NOTICE  [sshd] Unban 203.0.113.66
```

* `Found`, `Ban`, `Increase Ban <ip> (<n> # <dur> -> <end>)` (after the
  `Ban`, like fail2ban's observer), `Restore Ban` (state restored at start),
  `Unban`: exactly fail2ban's message shapes, so `grep`, `diff` and
  fail2ban log tooling work on both files.
* `galion-guard.shadow` lines carry what fail2ban has no line for (end time
  with offset, failure count, unban reason, non-IP hosts, errors).
* **Protected addresses are never written** (owner allowlist, self, recent
  login): the line says `<protected address withheld>` and the reason.
* fail2ban stamps carry no offset. The VPS fail2ban writes the system zone
  (UTC); the shadow log writes Europe/Zagreb by default (`shadow_zone`).
  The reader takes the zone of each file as a flag. In the one ambiguous
  autumn hour (2026-10-25 02:00-03:00) a Zagreb stamp is read as CEST.
* File mode 0600, opened per write (logrotate rename needs no signal;
  staged `deploy/staged/galion-guard-shadow.logrotate`). Volume on the VPS
  ~70 KB/day.

## 4. `galion-guard compare` (side-by-side report)

```text
galion-guard compare <fail2ban.log> <shadow.log> [--fail2ban-zone utc|zagreb] [--shadow-zone zagreb|utc]
                     [--tolerance SECS] [--since RFC3339] [--until RFC3339] [--mask] [--timeline] [--json]
```

* Window: the overlap of the two logs (or `--since/--until`).
* Bans (not restores) get their ban number from the following `Increase
  Ban`. Each fail2ban ban is paired with the nearest unused shadow ban of
  the same address within `--tolerance` (default 5 s); twins just outside
  the window are not counted as differences.
* Reports per side: failures and failing addresses, bans and banned
  addresses, increased bans, ban-number histogram, unbans of bans in the
  window, unbans of earlier bans, restores; then matched same-second /
  within-tolerance, max delta, bans only on one side, ban-number
  mismatches, addresses with a different failure count (listed).
* `--timeline` prints every ban in two columns (Zagreb times with offset)
  with `=`, `-1 s`, `ONLY F2B`, `ONLY GUARD`. `--mask` replaces addresses
  with `ip-1`, `ip-2` ... for pasting into docs. `--json` for machines.
* **PARITY** = no one-sided bans, no ban-number mismatch, identical failure
  counts per address. Exit 0 = parity, 3 = differences, 1 = error, so a
  timer can alert on it.

`galion-guard replay <log> --shadow-log <file>` writes the same shadow log
from a historical log, so the comparison can be run on the past before any
daemon is installed (that is the evidence in section 5).

## 5. Evidence: the VPS's last 2 days 8 hours

On 2026-10-21 04:41 +02:00 the VPS journal for fail2ban's own match
(`_SYSTEMD_UNIT=sshd.service + _COMM=sshd`, since fail2ban's restart
2026-10-18 20:10:05 +02:00; 33 230 lines) and `/var/log/fail2ban.log`
(1 678 lines) were copied read-only into a 0700 temp dir on the box,
replayed with `--shadow-log`, compared, and deleted at once. Only the masked
report is kept (docs/real-tests/guard-shadow.md).

| Metric (window 2026-10-18 20:15:49 +02:00 .. 2026-10-21 04:30:08 +02:00) | fail2ban 1.0.2 | galion-guard shadow |
|---|---|---|
| failures (Found) | 983 | 983 |
| failing addresses | 185 | 185 (per-address counts identical) |
| bans | 121 | 121 |
| banned addresses | 108 | 108 |
| increased bans | 13 | 13 |
| ban numbers | #1 x108, #2 x7, #3 x3, #4, #5, #6 | identical |
| unbans of these bans | 116 | 116 |
| bans matched | - | 44 same second, 77 at -1 s (76) / -2 s (1) |
| one-sided bans / ban-number mismatches | - | 0 / 0 |
| **verdict** | | **PARITY** |

The -1 s deltas are replay artefacts: a replayed ban is stamped with the log
line's whole second, fail2ban's with its wall clock a few hundred ms later
(a live shadow daemon stamps wall-clock time too). fail2ban's 4 extra
"unbans of earlier bans" lift bans restored from its database at its
2026-10-18 restart, before the window; they are reported, not compared.
The staged config (`protect_accepted = "24h"`) gave the identical report:
no key login came from an address that also failed in this period.
Our wider journal match (adds `ssh.service` and `sshd-session`) returned
exactly the same 10 065 lines as fail2ban's match over the last 24 h, so the
live shadow sees what fail2ban sees.

## 6. Rollout (needs Slavik's approval; nothing done)

Phase 0 is done (this branch). Each later phase is one owner decision.

| Phase | What | Go / no-go |
|---|---|---|
| 0 Past replay | `replay --shadow-log` + `compare` on the real journal (section 5) | done: PARITY over 2 d 8 h, 121 bans |
| 1 Live shadow | Install the x86_64-musl binary to `/usr/local/bin/galion-guard`, `/etc/galion-guard/guard.toml` from `deploy/staged/galion-guard.toml` (**backend = "shadow"**) plus the owner's address from `jail.d/galion-sshd.local` in `ignoreip`, the unit, the shadow drop-in and the logrotate file. `systemctl enable --now galion-guard`. fail2ban keeps protecting the host. | Daily `galion-guard compare /var/log/fail2ban.log /var/lib/galion-guard/shadow.log --fail2ban-zone utc` = exit 0 for 7 days (>= ~350 bans at today's rate), 0 `write_errors`, RSS < 64 MB, 0 restarts. Any difference is explained or fixed before phase 2. |
| 2 Cutover | `systemctl stop fail2ban && systemctl disable fail2ban`; remove the shadow drop-in; set `backend = "nft"`; `systemctl daemon-reload && systemctl restart galion-guard`. The state file and journal cursor carry over, so the shadow's live bans (the same as fail2ban's, phase 1) go into the kernel at start with their remaining time and no journal line is missed. Check `nft list table inet galion-guard` and `galion-guard status`. | Bans appear in the kernel set; the owner can still log in (allowlist accept rule precedes the reject rule). |
| 3 Watch | 7 days of `galion-guard status`; keep fail2ban installed but disabled. | No lock-out, ban rate in line with phase 1. |
| Rollback (any phase) | Phase 1: `systemctl disable --now galion-guard` (nothing to undo in the firewall). Phase 2/3: `systemctl stop galion-guard && galion-guard teardown && systemctl enable --now fail2ban` (fail2ban restores its own bans from its database). | - |

Nothing in phase 1 can lock anyone out: the shadow backend has no firewall
path and, with the drop-in, no capability to get one.

## 7. Owner decisions (Slavik)

1. **Start phase 1 (live shadow on the VPS)?** Options: (a) yes, as in
   section 6; (b) more replay-only comparisons first (no install); (c) not
   now. Evidence: section 5 (PARITY on 121 real bans). Phase 1 installs a
   binary, a config, a unit and a drop-in, so it is a VPS change that needs
   approval.
2. **Length of the shadow phase**: proposed 7 days or ~350 bans, whichever
   is later.
3. Still open from docs/guard-rust.md: `bantime_maxtime` (fail2ban has no
   cap; the 6th ban today is 32 h; recommended `1w`), `protect_accepted`
   (24 h, not in fail2ban; `"0"` for strict parity; both gave the same
   result in section 5), counting the OpenSSH 9 `Disconnecting
   authenticating user ... Too many authentication failures` form (stock
   parity today).

## 8. Tests

* Unit (lib): 59 (+9: fail2ban log parse/format/zones/junk, compare parity
  across zones / differences / ban-number + tolerance / window edges,
  shadow backend, config).
* `tests/shadow.rs` (2): engine in shadow mode over a 3-round attack corpus
  vs a fail2ban-layout reference written independently in the test (UTC,
  `fail2ban.*` loggers) -> PARITY, and a removed ban -> `ONLY GUARD`;
  restart writes `Restore Ban`, no firewall errors.
* `tests/real_shadow.rs` (GALION_REAL=1, release binary, section 2):
  passes; output in docs/real-tests/guard-shadow.md.
* Existing: 9 replay, 2 fuzz, 1 real-daemon: unchanged and passing.
