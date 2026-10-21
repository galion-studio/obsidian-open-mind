<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: vps-preflight-scripts (slot 195, galion-flight)

Branch swarm/vps-preflight-scripts on integration-4 42de672. All times Europe/Zagreb (+02:00).
Tests: `crates/galion-flight/tests/real_flight.rs` (Rust, `GALION_REAL=1`; no shell scripts).
Full transcripts: [vps-run-1608.log](vps-preflight-scripts/vps-run-1608.log),
[box-run-1607.log](vps-preflight-scripts/box-run-1607.log).

## 1. Preflight READ-ONLY against the live VPS

Command: `GALION_REAL=1 GALION_FLIGHT_VPS=slavik-vps cargo test -p galion-flight --test real_flight real_vps_flight -- --nocapture`
(the test runs the real `galion-flight` binary 17 times). Runs: 15:59-16:01 and 16:08:03-16:09:55, both pass.
The first manual pass 15:57:20-15:58:00 (release build) gave the same results.

| Run 16:08 | exit | checks | remote reads | time | fails |
|---|---|---|---|---|---|
| preflight nerve | 0 GO | 18 | 15 | 8.7 s | - |
| preflight memory | 0 GO | 16 | 13 | 7.7 s | - |
| preflight hermes-rs | 0 GO | 17 | 14 | 8.1 s | - |
| preflight galion-one | 0 GO | 17 | 13 | 8.2 s | - |
| preflight tuner | 0 GO | 16 | 13 | 7.5 s | - |
| preflight cloudflared | 1 NO-GO | 21 | 15 | 10.3 s | `nerve.view cloudflared` (failing-soon, pre-existing) |
| postflight nerve | 0 GO | 13 | 10 | 6.8 s | - |
| postflight memory | 0 GO | 11 | 8 | 5.7 s | - |
| postflight hermes-rs | 0 GO | 12 | 9 | 6.2 s | - |
| postflight galion-one | 0 GO | 13 | 9 | 6.9 s | - |
| postflight tuner | 0 GO | 12 | 9 | 5.9 s | - |
| postflight cloudflared | 1 NO-GO | 16 | 10 | 7.9 s | `nerve.view cloudflared` |

Postflight was also run against the live VPS (read-only; no change happened, so it must pass with
today's values). Negative controls, still read-only: `--set sha256=0…0` on nerve -> NO-GO on
`exe.running-sha256`; `quiet_timer=galion-backup.timer 999999` on tuner -> NO-GO on exactly
`timer.quiet`; `postflight memory --since-unix 4102444800` -> NO-GO on exactly `unit.restarted`;
unknown service -> exit 2; `--set exe=/opt/x;id` -> exit 2 (rejected before anything is sent).

Every command sent is printed (`--show-commands`) and the test asserts each starts with one of
`systemctl show`, `readlink`, `sha256sum`, `stat`, `ss -ltnH`, `curl -sS` and has no `-X`,
`--data` or `Environment`. Distinct command shapes sent (run 16:08): `systemctl show
--timestamp=unix -p ActiveState -p SubState -p Result -p NRestarts -p MainPID -p
ExecMainStartTimestamp -p DropInPaths -p UnitFileState '<unit>'`, `systemctl show
--timestamp=unix -p NextElapseUSecRealtime 'galion-backup.timer'`, `readlink '/proc/N/exe'`,
`sha256sum '<path>'`, `stat -c '%s %a %U:%G %Y' '<path>'`, `stat -f -c '%a %S' '/opt/galion'`,
`ss -ltnH 'sport = :N'`, `curl -sS -m 5 --max-filesize 4194304 -w '\n%{http_code}'
'http://127.0.0.1:N/<path>'`.

### What the live VPS showed (16:08 +02:00)

- No drift: every running image matches CALL-RECORD (nerve/memory/hermes aa77864 from 09:43-09:46,
  galion-one 40454f9 from 08:39, tuner 42de672 from 04:32, cloudflared unchanged since 2026-09-21);
  running image = file on disk for all six; NRestarts 0 everywhere; drop-ins exactly as recorded.
- Every rollback target is present with its recorded sha (galion-nerve 3b150dea…, openmind-memory
  473ae4a3…, hermes-gateway cd344b16…, galion-one.old 090c15d0…, tuner backup ecad72a4…,
  cloudflared config e3825ce4…) and the unit backups exist.
- galion-backup.timer next fires 2026-10-21 20:21:43 +02:00; /opt/galion 76,142 MiB free.
- cloudflared binary is mode 775 (D-195-3). The nerve rates cloudflared failing-soon (D-195-2).

### Nerve observed before/after (cortex is not deployed; the nerve is the observer)

| | ts | cycle | counts | score | PIDs / NRestarts |
|---|---|---|---|---|---|
| before | 16:07:54 | 769 | healthy 11, degrading 1, failing-soon 1, failing 0, absent 2 | 61 D | nerve 87026/0, memory 88565/0, hermes 89484/0, one 29782/0, tuner 4004553/0, cloudflared 22835/0, tuner-quick 1198838/0 |
| after | 16:09:54 | 773 | same | 61 D | identical |

The 61/D grade and its 3 open incidents (mouth stuck loading, tunnel to closed ports, hermes
spend cap) are pre-existing and unrelated.

## 2. Postflight against box instances

Command: `GALION_REAL=1 cargo test -p galion-flight --test real_flight real_box_postflight -- --nocapture`
with release builds of 42de672 + this branch (hermes-gateway built like live: `--no-default-features
--features sqlite`). Runs 16:01:45, 16:07:40, 16:07:43, 16:07:46: 4/4 pass, ~2.2-2.4 s each.

Instances (real binaries, temp dirs, loopback): openmind-nerve 20650, openmind-memory 20651,
hermes-gateway --mock 20652, galion-one 20653 (+ mouth 20659), tuner 20654; cloudflared stand-in
/ready on 20655/20656 (in-process; a real cloudflared on the box would publish a URL).

The SHIPPED live manifest is used for each service, re-pointed at the box instance with `--set`
(unit empty, port, exe, sha, owner, no nerve view). Per run:

- 5/5 services GO (6-9 checks each: exe path, running sha, file sha, stat, port, every GET);
  cloudflared stand-in GO, body (with a fake connector id) not shown.
- Wrong sha: 5/5 NO-GO on `exe.running-sha256` + `exe.file-sha256`.
- Wrong body expectation: 5/5 NO-GO.
- Live galion-one mouth line (`503 json:status=loading`) against the 42de672 build: NO-GO on exactly
  `http.get :20659/health` (42de672 answers `503 unavailable`, see D-195-4). Box nerve is dry-run,
  live is `mode=live`; the box run overrides that one line.
- Instance killed: 5/5 NO-GO (4-6 fails: running exe, port, GETs).

Ports 20650-20659 free afterwards; no processes left.

## 3. Unit tests and build

- galion-flight: 12 unit tests (command whitelist, argument validation incl. shell syntax,
  secret-property refusal, one read per check, NO-GO cases, timer parsing fails closed, body
  judging never echoes bodies, nerve view parsing, builtin manifests parse).
- Workspace (excl. openmind-sandbox, cgroup-detect): 1239 pass / 0 fail / 6 ignored.
- fmt clean; clippy -D warnings (workspace all-targets) clean; no-python ok; spdx-scan 0 missing;
  repo-check PASS (0 errors; 1 new warn: `SystemTime::now()` in main.rs feeds the timer
  comparison only).
- x86_64 musl static (582,320 B, sha256 4688c530…aaef; ran preflight tuner against the VPS: GO),
  aarch64 musl static (ran `list` under qemu-aarch64), aarch64-linux-android and
  armv7-unknown-linux-musleabihf `cargo check` ok. ucpu-gate.sh not run: zig missing on the box.

## 4. VPS footprint

Nothing installed, copied, restarted or POSTed. One disclosure: during the manual reconnaissance
at 15:49 +02:00 (before the tool existed) one ssh command wrote curl output to `/tmp/.x` on the
VPS and removed it in the same command. galion-flight itself writes nothing on the VPS (bodies
go to stdout).
