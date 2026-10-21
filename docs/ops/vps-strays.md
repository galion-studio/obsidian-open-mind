<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# VPS stray listeners 8431-8433, 8652, 8653: dossier and recommendation

Wave 4 slot 117 (`swarm/vps-strays`). Read-only probes on 2026-10-21 at 02:35, 02:39 and 02:49 CEST (+02:00).
I stopped nothing, restarted nothing and wrote nothing on the VPS. The VPS clock runs in UTC, so every time below has been converted to CEST.
Earlier records: vps-baseline (2026-10-20 22:52 CEST) and tunnel-audit (2026-10-20 ~20:35 CEST).

## 1. Summary
- **Owner.** All five are root processes. They were started by hand from SSH sessions on 2026-10-18 between 22:14 and 22:32 CEST, during the "growth" benchmark of branch `vps/ws-growth` (commits 3b3e3be, c6f0781 and 9661ef7, all stamped 22:32:14 CEST).
  The binaries contain `/home/box/.cargo/registry` paths, so they were built on the swarm box and copied up.
  Those 5 SSH sessions are logged out (`loginctl` State=closing). Their scopes stay open only because these processes still run. Nothing supervises them, nothing logs them and nothing restarts them.
- **Traffic.** None. 0 established connections in every sample, no unit, cron, nginx or cloudflared file names their ports, and nothing has written to their data dirs since 2026-10-18 22:32 CEST.
- **Leak?** While idle, memory is flat in all three samples. The 30-53 MB held by the mock gateways **is a real leak in `--mock` mode**, though: `MockProvider` clones every chat request into an unbounded `calls` Vec, which is a test-recording feature.
  On the box, the same bench load grew a mock gateway by about 5.5 MB per round, in a straight line. With the fix on this branch it stays at about 4.9 MB (section 4).
  The live `hermes-rs` on 8642 runs without `--mock` (CurlProvider), so the leak does not affect it: RSS 4.3 MB.
- **Recommendation: stop all five** (owner action, one approval for each port, order in section 5). Keep nothing running. Archive the bench results first.

## 2. Per-process dossier

| Port | PID | Binary (sha256 of running bytes) | Started (CEST) | SSH session | RSS kB: 10-20 22:52 / 10-21 02:35 / 02:49 | VmHWM kB | CPU over 2 days | What it is |
|---|---|---|---|---|---|---|---|---|
| 8431 | 1375169 | `/opt/galion/rust/bin/openmind-memory`, `473ae4a3…` = live 8420 unit binary | 10-18 22:14:52 | 48893 | 4040 / 4040 / 4040 | 4744 | 61.2 s | "shadow-old" memory, data `shadow-old/` (9.7 MB) |
| 8432 | 1412682 | `bin/openmind-memory.new` **(deleted)**, `e75ddab7…` | 10-18 22:23:51 | 49009 | 6728 / 6728 / 6728 | 7056 | 7.7 s | "shadow-new" memory (read-write), data `shadow-new/` (9.0 MB) |
| 8433 | 1428048 | same deleted `e75ddab7…` | 10-18 22:24:14 | 49012 | 3304 / 3312 / 3312 | 3996 | 7.8 s | read-only + recall-cache variant over `shadow-new/` |
| 8652 | 1395619 | `/opt/galion/rust/bin/hermes-gateway --mock`, `cd344b16…` = live hermes-rs unit binary | 10-18 22:15:53 | 48941 | 53172 / 53172 / 53172 | 53668 | 14.8 s | "hermes-old" bench gateway, state `hermes-old/` (1.6 MB) |
| 8653 | 1443105 | `bin/hermes-gateway.new --mock`, `a68271da…` (still on disk, same sha) | 10-18 22:32:28 | 49065 | 29820 / 29820 / 29820 | 30488 | 37.9 s | "hermes-new" bench gateway (`vps/ws-growth` 3b3e3be: queue, watchdog, /metrics), state `hermes-new/` (0.9 MB) |

All five bind 127.0.0.1 only and answer `GET /health` with 200 (278/278/277/122/122 B).
Their CPU use now is effectively zero. In 203 s, 8431, 8432, 8433 and 8652 used 0 ticks; 8653 used 3 ticks (about 0.015 %), from its `hermes-watchdog` thread.
8431's lifetime `rchar` is 13.1 GB, against 90 MB for 8432. That traffic is the old recall path re-reading its DB during the bench (old recall 28.6 ms p50, new 1.25 ms p50). `rchar` did not move between samples.
Earlier figures match: tunnel-audit at ~20:35 CEST on 10-20 recorded 52 MB and 29 MB, and hermes-core at 21:33 recorded 53 MB.

### /root/growth-20261018/bin (the `.new` builds)
| File | Size | mtime (CEST) | sha256 | Running? | Source |
|---|---|---|---|---|---|
| hermes-gateway.new | 2,195,392 | 10-18 22:32:24 | `a68271da…db510` | yes, 8653 | `vps/ws-growth` 3b3e3be (its log line has the new `max_conns/max_runs/queue` fields) |
| openmind-memory.new | 2,026,368 | 10-18 22:32:24 | `1c35a0d6…df8f0` | **no**. 8432/8433 run an earlier, now deleted build `e75ddab7…` started 22:23 | `vps/ws-growth` c6f0781 (the on-disk copy is the final rebuild) |
| galion-bench | 490,696 | 10-18 22:32:24 | `9a2268fa…51984` | no | `vps/ws-growth` 9661ef7 (std-only load generator) |
| gaia | 712,424 | 10-18 22:35:38 | `a0431fdb…0cce3` | no | gaia CLI, not attributed |

`vps/ws-growth` is **not** in GitHub main 13a296c. None of these four builds matches a shipped unit.
Bench results next to them, which are worth keeping: on the new memory build, recall was 4747 rps vs 255 rps and capture 3176 rps vs 118 rps. On the new gateway, chat_c12 got 0 429s against 88-197 for the old one.

## 3. Risks of leaving them running
1. **Port 8652 is taken.** `deploy/staged/hermes-rs.service`, the planned shadow, binds 8652 and fails with "address in use" while 1395619 lives.
2. **Unknown builds with live credentials.** The two hermes strays hold `API_SERVER_KEY`, and all three memory strays hold an `OPENMIND_MEMORY_API_KEY` (names only, from tunnel-audit; the values were not read).
   8432/8433 run bytes that no longer exist anywhere: a deleted inode that matches no commit.
3. **No supervision.** They will not come back after a reboot. nerve/cortex count them as 5 unmanaged listeners (the vulnerability finding).
4. **Memory is small but dead weight:** about 97 MB RSS in total, 83 MB of it in the two mock gateways' leaked request logs.
5. **No spend risk.** Neither hermes stray has `OPENROUTER_API_KEY` or `RIVER_API_KEY`, both run `--mock`, and `spend.json` has not been written since 22:32 on 10-18.

## 4. Root cause of the mock-gateway memory (box proof)
`crates/hermes-rs/src/provider.rs`: `impl Provider for MockProvider` does `self.calls.lock().push((target.clone(), req.clone()))` on **every** chat.
The tests need that Vec, but `hermes-gateway --mock` is meant to be a long-running shadow, and there it never stops growing. `vps/ws-growth` has the same line.
The real test `crates/hermes-gateway/tests/real_mock_rss.rs` (`GALION_REAL=1`, port 19170) replays the VPS bench mix 6-8 times:

| Build (x86_64 musl, release) | RSS after rounds 1..8 (kB) | Per-round growth |
|---|---|---|
| main 13a296c, unpatched | 8292, 14148, 19872, 25428, 30952, 36476, 41904, 47120 | +5.2 to 5.9 MB, linear (~9 kB per successful chat) |
| this branch (`with_record_limit(0)` in the gateway) | 3192, 3680, 4060, 4360, 4672, 4864, 4856, 4908 | levels off (+52 kB in the last round) |

On the VPS, 8652 served about 3.3k successful chats and holds 53 MB; 8653 served about 1.8k and holds 30 MB. That fits the same per-chat retention, plus the larger musl/VPS heap overhead.
The fix is a separate commit: `MockProvider::record_limit`, which defaults to unlimited so tests are unchanged, plus `with_record_limit(0)` in `hermes-gateway --mock`. A unit test `mock_record_limit_bounds_calls` covers it.
It does not need a VPS deploy for the strays, since stopping them frees the memory. It **does** matter before `deploy/staged/hermes-rs.service` (the `--mock` shadow) is installed.

## 5. Recommendation: stop all five, keep none (owner decision; NOT done)
Order: lowest risk first, one port per approval.
| Step | Port(s) | Why it is safe | Rollback |
|---|---|---|---|
| 0 | - | Copy `/root/growth-20261018/*.txt`, `*.log` and `bench-*.sh` into the repo record (the bench evidence) | n/a |
| 1 | 8432 + 8433 (`kill 1412682 1428048`) | Deleted bytes, no clients, data dir unchanged since 22:23 on 10-18 | Cannot relaunch the same bytes. The on-disk `.new` (c6f0781) is the better build anyway |
| 2 | 8653 (`kill 1443105`) | Mock, no clients, leaking | `bin/hermes-gateway.new --mock` with the same env, binary still on disk |
| 3 | 8431 (`kill 1375169`) | Duplicate of the live 8420 binary over a copy dir | Relaunch from the recorded argv |
| 4 | 8652 (`kill 1395619`) | Mock duplicate of live hermes-rs; frees 8652 for the staged unit | Install the staged unit instead of relaunching by hand |
| 5 | - | `mv /root/growth-20261018 /var/backups/galion/growth-20261018` (21 MB). Do not delete it | Move it back |

Use a plain SIGTERM. Check after each step with `ss -ltn` that only that port has gone, and that 8420, 8642 and every unit are unchanged (`systemctl status openmind-memory hermes-rs`).
These processes are outside system.slice, so systemd restarts nothing. A shadow that is still wanted should run from a unit, with the leak fix in place.

## 6. How it was measured (read-only)
`ssh … 'nice -n 10 bash -s'` heredocs running: `ss -ltnpH`; `ps -o lstart,etime,rss,…`; `/proc/<pid>/{status,smaps_rollup,stat,io,cgroup,exe,cwd}`; `ls /proc/<pid>/fd | wc -l`; `sha256sum /proc/<pid>/exe` and the `bin/` files; `ss -tanH` state counts; `loginctl show-session -p State,Remote,Timestamp`; `grep -rl` of the port numbers in /etc/systemd, /etc/cloudflared, /etc/nginx and cron (names only); `cat` of the bench logs, results and scripts in /root/growth-20261018; `curl GET /health` ×5; and `strings | grep cargo/registry` on the binaries.
`/proc/<pid>/environ` and the secrets dirs were not read. Nothing was written to the VPS.
