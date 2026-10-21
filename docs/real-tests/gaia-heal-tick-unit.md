<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: staged Gaia heal-tick unit (wave-4 slot 188)

Branch `swarm/gaia-heal-tick-unit`, base `swarm/gaia-loop-decisions` b40208a. All times are Europe/Zagreb CEST (+02:00), 2026-10-21.
Scope: box only, plus read-only VPS probes. **Nothing was installed (D-L1).** Ports: 20580 (fake nerve), 20581 (real `openmind-nerve serve`), 19880 (canary). Gaia's apply guard only allows 19000-19999, so the canary sits there, as in slot 175.

## What is tested

| test | gate | what |
| --- | --- | --- |
| `crates/gaia/tests/staged_heal_unit.rs` (8 tests) | always runs | The unit's `ExecStart=`, expanded for `gaia-canary` the way systemd does it (`%i`, `${VAR}`, bare `$VAR` word-split), parses with `gaia::heal_loop::parse_cli`: 30 s interval, 5 s jitter, 1 h runtime, 5 errors, 3 restarts, OS jitter (no seed), stop file, token dir (no `--token-file`), live nerve 127.0.0.1:8670 (D-L2), gate config, state dir, target 19880. The canary passes `check_target`. The same unit pointed at 8642, 8600-8700 or `systemctl restart hermes-rs` is refused by scope (not for "program missing"). `ConditionPathExists=!` = `--stop-file`. Lifecycle (`Type=exec`, `User=gaia`, `Restart=no`, `KillMode=process`, no `[Install]` on the service, no `Requires=`, `StateDirectory` list, no `ReadWritePaths`). 28 hardening keys and values. Timer (`Unit=`, 5 min re-arm, `[Install]`). SPDX + STAGED marker on all 5 files. Canary plan sha256. |
| `crates/gaia/tests/real_heal_tick_unit.rs` `real_staged_unit_exec_start` | GALION_REAL=1, GAIA_NERVE_BIN, optional GAIA_VPS_STATE | Runs the **unit's own ExecStart** on release builds, phases 1-7 below |
| same file, `real_systemd_analyze_verify` | GALION_REAL=1 | `systemd-analyze verify --root=<tmp>` (box systemd 257) on the template instance and timer: exit 0, **no output**. Negative control (`KillMode=bogus`) is reported. `systemd-analyze security --offline=yes`: exposure ≤ 2.0 (measured **1.1 OK**) |

Run:
```
export CARGO_TARGET_DIR=/workspace/swarm/target/gaia-heal-tick-unit
cargo-slot build --release -j 2 -p gaia -p openmind-nerve
GALION_REAL=1 GAIA_NERVE_BIN=$CARGO_TARGET_DIR/release/openmind-nerve GAIA_VPS_STATE=/tmp/s188/vps-state.json \
  cargo-slot test --release -j 2 -p gaia --test real_heal_tick_unit -- --nocapture
```

Rewrites made to the unit argv for the box (and nothing else): `/var/lib/gaia` and `/etc/galion` move under a temp root, `127.0.0.1:8670` becomes 127.0.0.1:20580, `/opt/galion/rust/bin/{gaia,gaia-dummy}` become the release builds, and `--interval-ms 30000 --jitter-ms 5000` become 1000 / 200. The test asserts that no `/opt`, `/var/lib` or `/etc` path is left.

## Results: `real_staged_unit_exec_start`, 6 logged passing runs (13:33-13:37) + 1 at 13:32

Run 5 (13:36:53-13:37:15), as printed:

| phase | systemd equivalent | result |
| --- | --- | --- |
| P1 | timer starts the loop; canary down; 1 admin-minted token | healed in 100 ms (all 6 runs), token `.tok.spent`, ledger consumed 1, `gaia.heal.applied` 1, child log at `var/lib/gaia/gaia-canary/state/gaia-canary.log` |
| P2 | `systemctl stop` with `KillMode=process` = SIGTERM to the gaia main pid only | gaia gone in 20 ms (killed by SIGTERM, all 6 runs). Canary pid 1343842 is in its own group (pgid 1343842; gaia's was 1342792) and **still answers with the same pid** |
| P3 | timer re-arm (new loop) | 3+ healthy ticks, no token used, still 1 consumed |
| P4 | D-L2: nerve `/state` `restarts_1h=2` (+ reflex `live_1h=2`), Gaia 1 in the global ledger, canary killed | `gaia.heal.capped`; consumed still 1; 2 tokens kept; canary stays down. Nerve set back to 0: healed in 701-801 ms; consumed 2; `applied` 2 |
| P5 | `touch /var/lib/gaia/gaia-canary/stop` | loop exit 0, `stop: stop-file`, in 20-100 ms (6 runs: 20, 40, 60, 60, 80, 80, 100). The canary keeps running. A loop started while the file exists stops before its first tick, exit 0 (systemd would skip the start through `ConditionPathExists`) |
| P6 | nerve `/state` returns 500, canary down | fails closed: exit **7** `max-errors` after 15.3-15.7 s, back-off sleeps `[1142,2133,4061,8120]` ms (with the unit's 30 s interval that is 30/60/90/90 s, about 4.5 min, then the 5-min re-arm). **0 tokens consumed**, canary not started |
| P7 | real `openmind-nerve serve` on 20581 | `gaia nerve-restarts` parses its `/state`: `gaia-canary` count 0, 3 left |
| P7 | live VPS `/state` (saved read-only GET, 13:31:57, cycle 457, ts 13:31:54) | parses: gaia-canary 0 (not watched); galion-one, cloudflared, hermes-rs `restarts_1h` 0 / `live_1h` 0, so 3 left each; galion-nerve `restarts_1h` 0 (no reflex budget row) |
| write set | `ProtectSystem=strict` + `StateDirectory` + `ReadOnlyPaths=/etc/galion` | every changed file is under `var/lib/gaia`: the child log, `state/restarts.jsonl`, the token files (`.tok`, `.spent`, request/plan/minted from `gaia tokens request`), `hades/gaia-restarts.jsonl` + `.lock`, `hades/ledger.jsonl`. `/etc/galion` is byte-identical |
| secrets | | no `hades1.` token string in any loop output, CLI output or nerve event |
| teardown | | real nerve, loops (matched by temp dir in cmdline) and canary killed; 20581 and 19880 free; temp dir removed |

The first 3-run batch (13:33-13:34) found a test bug, not a unit bug. A run that failed an assert left its own gaia loop running (it was not in the Guard). That loop then healed the next run's canary, and the next run counted a second `applied`. Also, `.tok` was counted before the loop renamed the token (a race). Fixed: the Guard now kills every loop it started (matched by temp dir), and the token count waits. Then 5/5 passed in a row.

## systemd-analyze (box, systemd 257.13-1~deb13u1, no systemd PID 1)

- `verify --root=<tmp> gaia-heal-tick@gaia-canary.service gaia-heal-tick@gaia-canary.timer`: exit 0, empty output, at 13:40:00 and again by hand. At debug level the only notes are libseccomp names inside `@system-service` that this libseccomp does not know (`llseek`, `fstatat`, ...). The box's own systemd-journald.service gets the same notes.
- Negative control: `KillMode=bogus` → `Failed to parse kill mode specification, ignoring: bogus`.
- `security --offline=yes`: **1.1 OK**, 73 checks passed. Open items: RootDirectory, AF_UNIX/AF_INET allowed (needed for loopback), ProcSubset (needed for /proc/net), PrivateNetwork (needed for the host-loopback nerve), PrivateUsers, char-rtc read, IPAddressAllow localhost.
- **Not testable on the box:** the box has no systemd PID 1 (tini), so the timer, conditions, seccomp filter and namespaces were not exercised live. On the VPS that would mean installing a unit, which D-L1 forbids. The cutover doc's verify step covers it.

## VPS, read-only (2026-10-21)

| time | probe | result |
| --- | --- | --- |
| 13:23:59 | `systemctl --version`, `list-units/list-timers 'gaia*'`, `ls /opt/galion/rust/bin`, `systemctl show galion-nerve` | systemd 255 (Ubuntu), LXC (`systemd-detect-virt`), **no gaia unit, timer or binary**; nerve = `/opt/galion/rust/bin/openmind-nerve-aa77864 serve`, drop-ins 50-rust-bin + 60-nerve-eventlog |
| 13:23:59 | GET 127.0.0.1:8670/state, /health | cycle 441, interval 30 s, mode live, score 61/D (galion-one-mouth stuck loading 3.7 h); /health 200 |
| 13:24 | `systemctl cat hermes-rs` (hardening lines) | live hermes-rs runs with ProtectSystem=strict, PrivateTmp, MemoryMax=64M in this LXC |
| 13:27 | `ss -ltn` 19xxx | only 127.0.0.1:19120 listening; **19880 free** |
| 13:28 | grep of unit files for each hardening directive + `is-active` | PrivateDevices (galion-health, galion-nerve, openmind-memory active), ProtectProc=invisible (systemd-networkd active), IPAddressDeny=any (journald, logind active), SystemCallFilter (rsyslog, journald active), MemoryDenyWriteExecute (logind), ProtectClock/KernelLogs/RestrictNamespaces (networkd, resolved, logind), StateDirectory (galion-nerve, hermes-rs), CapabilityBoundingSet (galion-nerve, openmind-queue) |
| 13:29 | `systemctl show systemd-logind -p IPAddressDeny -p IPEgressBytes`; cgroup controllers | `IPAddressDeny=::/0 0.0.0.0/0`, `IPEgressBytes=[no data]`: IP accounting/BPF firewall probably not active in this LXC (D-U5). Controllers: cpuset cpu io memory hugetlb pids rdma misc |
| 13:31:57 | GET /state saved to the box (/tmp/s188/vps-state.json, 45,156 B, not committed) | used in P7 above |

The cortex is not live (the nerve is the observation point, per the no_skip_extensive_testing rule). No VPS file was written and no state-changing request was made. A first `journalctl -b | grep` probe was too slow and was stopped from the box after 50 s. It was a read.

## Workspace

See the slot 188 DONE line in CALL-RECORD.md for fmt, clippy and workspace test numbers.
