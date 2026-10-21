<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-sandbox: real-life problem scenarios on the VPS (2026-10-21)

Owner Slavik, relayed by Mate. Branch `swarm/sandbox-real-problems`, based on
`swarm/realscript-port-3` 03967fc. All times are Europe/Zagreb, CEST +02:00.
The host is called "the VPS"; its address is not written here.

This is not the attack-containment suite (that is `docs/real-tests/sandbox.md`
and the 04:27-04:29 deploy-job run, 14 attacks contained). Here, **real work runs
inside the sandbox on copies of real VPS data**. Some runs do the job correctly.
Others are a realistic failure: a crash loop, a memory leak, a fork storm, a
hang, a tool reaching for the network or for secrets, a full disk. Every
scenario is judged twice: by the tool's own `RESULT` lines and by a host-side
check (root's count from the live data, a `/proc` scan for survivors, decoy
listeners, cgroup counters).

## Result (final run 3, 2026-10-21 05:10:02-05:10:23 +02:00, 21.4 s for all 10)

| # | Scenario | Verdict | Time | Key numbers |
|---|---|---|---|---|
| S1 | cloudflared 502 diagnosis on a masked copy of 24 h of journal | **PASS** | 94 ms | 54,888 lines, 27,439 failed requests (= 502s), 27,439 origin dial failures, 19 origins, 45 public hosts. **65/65 keys equal** to root's `grep` on the live journal |
| S2 | integrity of a COPY of the openmind-memory SQLite DB + WAL | **PASS** | 55 ms | 979 pages (995 with the WAL), 32 schema objects, **0 errors** in both views. WAL: 1013 frames, 236 valid, 14 commits. Both deliberate corruptions detected. Sandboxed output is identical to the unsandboxed output (67 lines) |
| S3 | crash-looping job under a restart-forever supervisor | **PASS** | 4.31 s | 104 crashes, 104/104 SIGABRT. Killed at the 4.007 s timeout. 0 processes left, cgroup removed |
| S4 | runaway memory (a leaky "cache" of journal lines) | **PASS** | 1.99 s | A: cgroup OOM kill (SIGKILL), oom_kills=3, memory.peak = **134,217,728 B = exactly 128 MiB**. B: RLIMIT_AS 128 MiB, dead on SIGSEGV after 235 ms |
| S5 | fork storm: 502 count that spawns 200 workers at once | **PASS** | 1.03 s | pids.max 24: 22 spawned, **178 refused (EAGAIN)**, pids.peak 24. The job fell back in-process and still got **27,439** (= host) |
| S6 | hung task + helper + setsid()-detached daemon | **PASS** | 7.01 s | Without a cgroup: wall 3.002 s. With the cgroup: 3.001 s. 3 tagged processes mid-run, **0 after** in both cases |
| S7 | tool reaching for live 8670/8672/8642/8650/8420, the internet, DNS | **PASS** | 6.57 s | 12/12 probes CONTAINED (ENETUNREACH / EADDRNOTAVAIL / resolver failure). Only `lo` is visible. Decoy listeners: **0 conns, 0 bytes, 0 datagrams**. The host-side positive control got through (+1 conn, +7 B, +1 datagram) |
| S8 | tool trying to read secrets (48 paths) | **FAIL (real finding)** | 48 ms | All real secrets were contained: galion-light/secrets, /root, /root/galion-secrets, /etc/shadow, /etc/*.env, 600/700 .env files, 7× /proc/<pid>/environ. **But `/opt/galion/galion-light/publish/obsidian/link.env` (mode 666) is readable** under the shipped preset and with no hiding. 10 world-readable `.env.example` templates are readable too (not counted as failures). The fix variant (preset + hide `/opt/galion`) brings this to **0 readable** |
| S9 | disk filling the workdir | **PASS + GAP** | 243 ms | a: RLIMIT_FSIZE 16 MiB, file capped at exactly 16,777,216 B (EFBIG, see note). b: **256 MiB written in 184 ms with no quota (GAP)**. c: 32 MiB tmpfs workdir, ENOSPC at 31 MiB |
| S10 | the known /proc gap, measured | **MEASURED (known gap)** | 51 ms | The sandbox sees 49 pids (the bare temp user sees 48, root counted 45 a moment earlier). Other users' cmdlines readable: **46**. Other users' environ readable: **0**. Cmdlines with secret-like words: **0** |

The sandbox held in 9 of 10 scenarios. S10 measures the known gap. The one
FAIL (S8) is not a broken sandbox layer. The sandbox has no Landlock on this
kernel, so any world-readable file on the host stays readable. The shipped
`read-only-tool` preset hides only `/root`, `/home` and `/run/secrets`.

Earlier runs at 05:05:51 (run 1) and 05:08:16 (run 2) found two bugs in the
runner's own judging, now fixed (see "What broke and was fixed"). Both used the
same setup, and each was cleaned up before the next run began.

## Setup (method copied from the 04:27 deploy-job run)

- Binary: one static x86_64-unknown-linux-musl binary, `tools/sandbox-real-problems`
  (this branch), built on the box with `cargo-slot ... -j 2`,
  `CARGO_TARGET_DIR=/workspace/swarm/target/sandbox-realprob` (deleted afterwards).
  The run-3 sha256 is `37ec242b63f107cf30907118e4a3b10fe3f4cffd168e4fdc030a7f004c76cd84`
  (865 KB, static-pie). It was never run as a sandbox on the box. The box only
  ran the three pure unit tests (masking, diag counting, SQLite garbage
  rejection) and the SQLite checker on local test DBs.
  - Host role: `srp run <base>` judges every scenario. `srp mask` masks IPs.
  - Sandbox role: the same binary is copied into each workdir as `./rp`. It runs
    the subcommands `diag502`, `dbsuite`, `crashloop`/`crashjob`, `memhog`,
    `forkjob`/`chunk`, `hang`/`detach`/`sleeper`, `net`, `secrets`, `fill`
    and `procview`.
  - There are no new third-party dependencies: `libc` (MIT OR Apache-2.0) is
    already used by openmind-sandbox. The crate has its own `[workspace]`
    table, so it is outside the root workspace and adds nothing to its build
    or CI.
- Throwaway dir: `/var/tmp/sandbox-test-20261021-050940` (run 3). `bin/` and
  `data/` are owned by root, 0755. `data/` and `w/` are chowned to the test user.
- User: `useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin sbxtest`
  (uid 996, gid 989). The runner refuses to start as root (`uid=996 euid=996`).
- Run: `systemd-run --wait --pipe --collect --unit=sbxtest-realprob-<ts> --slice=sbxtest.slice -p User=sbxtest -p Group=sbxtest -p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10 <base>/bin/srp run <base>`.
  The runner moves itself into `<unit cgroup>/sup` and writes
  `+cpu +memory +pids` to the unit cgroup's `cgroup.subtree_control`. It then
  passes the unit cgroup as `cgroup_parent`. That is the method that worked at
  04:28. The unit-level MemoryMax, TasksMax and CPUQuota are an outer safety
  net for the live services.
  - `CGROUP delegated parent /sys/fs/cgroup/sbxtest.slice/sbxtest-realprob-20261021-050940.service subtree_control=cpu memory pids`
  - `DETECT per_run_possible=true reason="per-run cgroup possible under /sbxtest.slice/sbxtest-realprob-20261021-050940.service: subtree already delegates cpu, memory, and pids"`
  - `FEATURES Features { landlock_abi: 0, userns: true, userns_full: true, mount_setattr: true, caps: true, android: false } landlock_abi=None`
  - `LAYERS rlimits=Enforced | env=Enforced | workdir=Enforced | no_new_privs=Enforced | userns=Enforced | net=Enforced | pidns=Enforced | mountns=Enforced | ro_root=Enforced | hide_home=Enforced | proc=NotEnforced("proc mount refused (e.g. container with overmounted /proc); host /proc stays visible") | landlock=NotEnforced("kernel has no landlock") | caps=Enforced | fds=Enforced | cgroup=NotEnforced(...)`.
    That line is for runs without `cgroup_parent`. In the runs with it, the
    `CGROUP` counters show the per-run cgroup was used.
- Policy: every run starts from the shipped `Preset::read_only_tool()` (256 MiB
  RLIMIT_AS, nofile 64, fsize 1 MiB, 15 s wall, network isolated and required,
  ro root, hide `/root` `/home` `/run/secrets`). Each scenario changes only the
  fields it tests.
- Data (copies only, written only inside the throwaway dir):
  - Journal: `journalctl -u cloudflared --since @S --until @U -o short-iso`, a
    fixed 24 h window from 2026-10-20 05:08:40 to 2026-10-21 05:08:40 +02:00,
    piped through `srp mask` running as sbxtest. 27,467 IPv4 addresses were
    masked to `X.X.X.X`, 127/8 and `::1` were kept, and 0 IPv6 addresses
    needed masking. The host-side grep afterwards found
    `j.masked_ipv4_left 0`. The 18.4 MB copy was deleted at cleanup.
  - DB: `cp` of `/var/lib/memory-tencentdb/instances/galion/vectors.db` and its
    `-wal`. The sha256 of the originals was the same before the copy, after the
    copy and after cleanup (`0ff84fad0d753fd3…`, `f50024d3b47a2837…`), and
    equal to the copies. The originals were never opened for writing.
  - Reference (`data/ref.txt`, 70 keys): computed by root from the live journal
    for the same window with `grep -c` / `grep -o | sort | uniq -c`. For the DB,
    `od` of the header gives the page size and page count, and `stat` gives the
    size.
  - Secret paths (48): a fixed list, `.env` / `*.env` / `.env.*` found by root
    with `find` (names and modes only, never contents;
    `/opt/galion/openmind` pruned), and `/proc/<MainPID>/environ` for
    galion-nerve, galion-nerve-shadow, hermes-rs, openmind-memory, cloudflared,
    tuner and pid 1. The sandboxed tool reads at most **one byte** of a file to
    prove it is readable, and prints only the word `ESCAPED`.
  - A 32 MiB tmpfs mounted by root on `<base>/w/s9c-tmpfs` (S9c), unmounted at cleanup.

## Scenarios: expected, actual, RESULT lines

### S1: cloudflared 502 diagnosis (PASS, 94 ms)
Expected: the tool exits 0 and every count equals root's count on the live
journal. Actual: 65/65 keys equal.
```
RUN ["diag502", "/var/tmp/sandbox-test-20261021-050940/data/journal.log"] status=Exited(0) timed_out=false wall_ms=94 error=None stdout_truncated=false
RESULT diag502 lines=54888 err_lines=54882 requests=27439 events=27439 first=2026-10-20T05:08:47+02:00 last=2026-10-21T05:08:38+02:00 ms=81
RESULT diag502 origin http://localhost:4001 7825      (top origins; all 19 in the appendix)
RESULT diag502 origin http://localhost:3011 7412
RESULT diag502 origin http://localhost:4020 3308
RESULT diag502 dest merlinelement.com 7285            (top public hosts; all 45 in the appendix)
RESULT diag502 dest galion.studio 6058
RESULT diag502 dest workspace3d.app 2073
HOST OK [S1] j.requests: sandbox 27439 vs host 27439
HOST OK [S1] j.events: sandbox 27439 vs host 27439
HOST OK [S1] j.lines: sandbox 54888 vs host 54888
HOST OK [S1] masked copy has 0 non-loopback IPv4 left (host grep)
```
Timestamps in the `first=`/`last=` fields were converted to +02:00 from the
journal's +00:00. What the data says: 99.99 % of cloudflared's lines in the
last 24 h are `ERR`. Every failed request is a dial to a closed local origin
port. The worst is `:4001` (galion.studio and merlinelement.com), then
`:3011` and `:4020`. This matches the nerve's open incident ("tunnel routes …
to closed origin ports").

### S2: SQLite integrity of the memory DB copy (PASS, 55 ms)
Expected: 0 structural errors with and without the WAL; the header matches
`od`; the sandboxed and unsandboxed results are identical; both corruptions
are detected. Actual: as expected.
```
RESULT dbcheck-main page_size=4096 header_pages=979 file_pages=979 pages=979 btree=874 overflow=104 freelist=1/1 ptrmap=0 orphans=0 objects=32 errors=0
RESULT dbcheck-wal page_size=4096 header_pages=979 file_pages=979 pages=995 btree=885 overflow=110 freelist=0/0 ptrmap=0 orphans=0 objects=32 errors=0
RESULT dbcheck-wal wal frames=1013 valid=236 commits=14 db_pages=995 distinct_pages=60 error=None
RESULT dbcorrupt-type root=30 errors=2 first=Some("page 30: bad b-tree page type 7 (expected table b-tree)")
RESULT dbcorrupt-trunc errors=2 first=Some("header says 979 pages, file has 978")
HOST OK [S2] sandboxed and unsandboxed checker output identical (67 lines)
HOST OK [S2] page_size: checker 4096 vs root od 4096
HOST OK [S2] header_pages: checker 979 vs root od 979
```
The checker (`tools/sandbox-real-problems/src/sqlite.rs`) is a pure-Rust
reader of the SQLite file format. It checks the header and the page count
against the file size. It walks every b-tree from `sqlite_schema` and checks
page types, cell pointers, rowid/key order, equal leaf depth and overflow
chains. It also walks the freelist and ptrmap pages, and checks that every
page is used exactly once. WAL frames are validated (salts plus cumulative
checksums), and committed frames are applied on top of the main file. It
reports schema names and entry counts only, never row contents. The VPS has
no `sqlite3` CLI, so the independent reference is root's `od` of the header
plus the two planted corruptions. The checker was also run on the box against
three local SQLite files, with 0 errors on each and both corruptions detected
on each. Finding: the live WAL holds 14 committed transactions (60 pages) that
have never been checkpointed into the main file (4.2 MB WAL, last write
2026-10-19 12:57). That is harmless, but `PRAGMA wal_checkpoint` on the next
maintenance window would shrink it.

### S3: crash loop (PASS, 4.31 s)
Expected: every child dies of SIGABRT and is restarted; the run is killed at
the 4 s wall timeout; 0 processes are left; the cgroup is removed.
```
RUN ["crashloop", ".../data/journal.log"] status=Signaled(9) timed_out=true wall_ms=4007 error=None stdout_truncated=false
OUT 104 progress lines, last: CRASH 104 sig=Some(6) code=None
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=19529728 (18 MiB) pids_peak=3 removed=true
HOST OK [S3] 104 crashes observed and restarted
HOST OK [S3] 104/104 children ended with SIGABRT
HOST OK [S3] 0 processes left in the workdir
```

### S4: runaway memory (PASS, 1.99 s)
Expected: A, memory.max 128 MiB (RLIMIT_AS 2 GiB), ends in a cgroup OOM kill
with peak <= 128 MiB. B, with no cgroup and RLIMIT_AS 128 MiB, fails to
allocate and dies on a signal.
```
RUN ["memhog", ...] status=Signaled(9) timed_out=false wall_ms=1754 error=None    (A)
OUT 4 progress lines, last: MEM iter=3 held_mib=70
CGROUP pids_max_hits=0 oom_kills=3 mem_max_hits=12825 mem_peak=134217728 (128 MiB) pids_peak=2 removed=true
RUN ["memhog", ...] status=Signaled(11) timed_out=false wall_ms=235 error=None     (B)
OUT 4 progress lines, last: MEM iter=3 held_mib=70
```
70 MiB of line payload is about 128 MiB of real memory once String and Vec
overhead is counted. The cap held to the byte. In B, Rust's
allocation-failure abort ended as SIGSEGV, not SIGABRT. Most likely the
abort path itself could not get memory under the 128 MiB address-space cap
(not verified further). It was still contained,
reaped, and left nothing behind.

### S5: fork storm during a real job (PASS, 1.03 s)
Expected: pids.max 24 refuses forks with EAGAIN; pids.peak <= 24; the job
falls back in-process and still reports the exact host count.
```
RESULT forkjob workers=200 spawned=22 spawn_failed=178 first_err="Resource temporarily unavailable (os error 11)" total_requests=27439 ms=1004
CGROUP pids_max_hits=178 oom_kills=0 mem_max_hits=0 mem_peak=410693632 (391 MiB) pids_peak=24 removed=true
HOST OK [S5] job total 27439 vs host 27439
```
(cgroup memory for this run was 512 MiB. Each of the 22 workers loads the
18 MB copy.)

### S6: hung task (PASS, 7.01 s)
Expected: the task is killed at the 3 s timeout (wall < 3.6 s); 3 tagged
processes are alive mid-run and 0 afterwards, with and without the cgroup.
```
RUN ["hang", "hang4040690nocg"] status=Signaled(9) timed_out=true wall_ms=3002
HOST OK [S6] nocg: 3 tagged processes mid-run (hang + helper + detached)
HOST OK [S6] nocg: 0 tagged processes after teardown
RUN ["hang", "hang4040690cg"] status=Signaled(9) timed_out=true wall_ms=3001
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=1384448 (1 MiB) pids_peak=5 removed=true
HOST OK [S6] cg: 0 tagged processes after teardown
```
Even without a cgroup, the PID namespace teardown kills the `setsid()` daemon.

### S7: network to live services and the internet (PASS, 6.57 s)
Expected: every connect, send and resolve fails; only `lo` is visible; the
decoys get 0 connections, bytes and datagrams; the host-side positive control
gets through. The live ports are only `connect()`ed, and no byte would be
written to them even if a connect succeeded.
```
RESULT net live-8670 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8672 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8642 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8650 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8420 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-v6-8670 CONTAINED Address not available (os error 99) ms=0
RESULT net internet-1.1.1.1:443 CONTAINED Network unreachable (os error 101) ms=0
RESULT net internet-8.8.8.8:53 CONTAINED Network unreachable (os error 101) ms=0
RESULT net decoy-tcp CONTAINED Network unreachable (os error 101) ms=0
RESULT net decoy-udp CONTAINED Network unreachable (os error 101)
RESULT net dns-udp-8.8.8.8 CONTAINED Network unreachable (os error 101)
RESULT net resolve CONTAINED failed to lookup address information: Try again
RESULT net interfaces 1 ["lo"]
HOST OK [S7] decoys during sandbox run: 0 conns, 0 bytes, 0 datagrams
HOST OK [S7] positive control reached decoys: +1 conn, +7 bytes, +1 datagram
```
Why "host listeners saw 0 bytes" also holds for the live ports: ENETUNREACH
comes from the route lookup inside the empty network namespace, before any
SYN exists. The decoys on host loopback prove this end to end. The live
services kept the same PIDs and 0 restarts, and the nerve saw no change
(below). The 5 s wall time is musl's resolver timing out against an
unreachable nameserver.

### S8: secrets (FAIL, a real finding; 48 ms)
Expected: every path CONTAINED or HIDDEN, with the preset and with
hide_paths emptied. A third run, the proposed fix (preset + hide
`/opt/galion`), must make every path unreadable.
```
RESULT secret /opt/galion/galion-light/secrets CONTAINED Permission denied (os error 13)
RESULT secret /root/galion-secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root HIDDEN listed 0 entries (empty cover)
RESULT secret /etc/shadow CONTAINED Permission denied (os error 13)
RESULT secret /etc/openmind-memory.env CONTAINED Permission denied (os error 13)
RESULT secret /proc/1466797/environ CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-light/publish/obsidian/link.env ESCAPED readable (1 byte read, not shown)
HOST FAIL [S8] preset: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]
HOST FAIL [S8] nohide: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]
NOTE [S8] preset: 10 world-readable .env.example templates readable (not secrets); preset: 36 contained, 1 hidden
NOTE [S8] nohide: 37 contained, 0 hidden
NOTE [S8] preset+hide-opt-galion: 0 world-readable .env.example templates readable (not secrets); preset+hide-opt-galion: 47 contained, 1 hidden
NOTE [S8] proposed fix (hide /opt/galion in tool presets) closes every readable path
```
Root's `stat` shows `link.env` is `666 root:root`. Its content was not read
and is not known. Two fixes, both for the owner to decide (nothing was changed
on the VPS):
1. `chmod 600 /opt/galion/galion-light/publish/obsidian/link.env`. It is a
   live path, so this is the owner's call.
2. Add `/opt/galion` (and `/var/lib`) to the `HIDE` list of the tool presets in
   `crates/openmind-sandbox/src/preset.rs`. Without Landlock, `ro_root` makes
   the whole host filesystem readable, and Unix permissions are the only
   barrier left. The third run shows that hiding `/opt/galion` alone closes all 11.

### S9: disk filling the workdir (PASS + GAP, 243 ms)
Expected: (a) one 64 MiB file under RLIMIT_FSIZE 16 MiB stops at exactly
16 MiB; (b) 32 × 8 MiB files are all written (no total quota: GAP); (c) on a
32 MiB tmpfs workdir, ENOSPC at about 32 MiB.
```
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=0 total_mib=16 err=File too large (os error 27)          (a, file = 16777216 B)
RESULT fill done files=32 total_mib=256                                         (b, 184 ms)
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=1925120 (1 MiB) pids_peak=2 removed=true
RESULT fill stop file=3 total_mib=31 err=No space left on device (os error 28)  (c)
```
- (a) The tool is **PID 1 of its PID namespace**. The kernel drops
  default-action signals sent to a namespace's init, so the kernel's SIGXFSZ
  never kills it and `write` returns EFBIG instead. The cap still holds to the
  byte. Side effect: a sandboxed tool also cannot be stopped by SIGPIPE or
  SIGTERM from inside the sandbox. The wall timeout's SIGKILL from outside
  still works (S3 and S6).
- (b) **GAP**: openmind-sandbox has only a per-file cap. Total workdir size is
  unbounded. The workdir is on ZFS, and ZFS's ARC is not charged to the
  memory cgroup (peak 1.8 MiB while 256 MiB were written), so `memory.max`
  does not slow a disk fill either. 76,674 MiB were free, so 256 MiB was safe
  to write, and it was deleted at once.
- (c) The mitigation works. A size-limited tmpfs (or a ZFS dataset with a
  quota) as the workdir stops the fill with ENOSPC. Proposal: give
  `SandboxPolicy` a `workdir_tmpfs_bytes` that mounts a private tmpfs inside
  the sandbox's own mount namespace. The `hide_paths` covers already use
  tmpfs mounts, so no host root would be needed.

### S10: the /proc gap, measured (MEASURED, known gap; 51 ms)
Expected (a known gap, measured): the host process list and cmdlines are
visible, the same as for the bare temp user; other users' environ and exe
links are not readable.
```
RESULT procview pids=49 other_uid=46 comm_readable=49 distinct_names=37 cmdline_readable=49 other_uid_cmdline_readable=46 environ_readable=1 other_uid_environ_readable=0 exe_link_readable=1 cmdlines_with_secretish_words=0
RESULT procview services_identifiable cloudflared=true galion-nerve=true galion-nerve-sh=true hermes-gateway=true openmind-memory=true sshd=true systemd=true tuner=true
RESULT procview file /proc/meminfo readable lines=55
RESULT procview file /proc/cpuinfo readable lines=216
RESULT procview file /proc/net/tcp readable lines=1
RESULT procview file /proc/1/environ denied Permission denied (os error 13)
BARE RESULT procview pids=48 other_uid=46 comm_readable=48 distinct_names=36 cmdline_readable=48 other_uid_cmdline_readable=46 environ_readable=2 other_uid_environ_readable=0 exe_link_readable=2 cmdlines_with_secretish_words=0
```
In practice, a sandboxed tool can list every host process, name every live
service, and read every cmdline (46 of other users). That is exactly what the
unsandboxed temp user sees, so the sandbox adds nothing here and takes nothing
away. Today 0 cmdlines contain a secret-like word (token, secret, passw,
api_key, apikey, bearer, --key). That is a property of how the services are
started, not a protection. Environ, `exe` and `/proc/net` (which is per
network namespace and shows only the sandbox's loopback) are not exposed.
Host memory, CPU and load info are readable. The fix (a private /proc) is
refused inside this Proxmox LXC because /proc is overmounted. Otherwise use
`hidepid=2` on the host /proc, or never put secrets in argv.

## What broke and was fixed during the run
1. The first prep (05:03:42, before any sandbox run) found `j.masked_ipv4_left 22`. 22 IPv4s inside
   URL-encoded SSRF probes (`url=http%3A%2F%2F<ip>%2Flatest`) were not masked,
   because the char before them is a hex digit. Fixed: `%XX` escapes are now
   treated as separators. A unit test was added, and the copy was remade
   (run 2 and later: 0 left). The unmasked copy never left the VPS and was
   deleted at the 05:05 cleanup.
2. S3 judging (run 1): Rust prints an ExitStatus as `unix_wait_status(6)`, not
   `signal: 6`. The payload now prints `sig=Some(6)`. That was a runner bug,
   not a sandbox bug.
3. S9a judging (run 1): the expectation said SIGXFSZ, but the kernel returned
   EFBIG because the tool is PID 1 of its namespace (see S9). The judging now
   accepts either. The real check, the file capped at exactly 16 MiB, held in
   every run.
4. S8: `.env.example` templates are now reported apart from real secrets,
   and the fix variant was added.
5. S10: the comm name `openmind-memory` is 15 chars, not `openmind-memor`
   (that was `ss` truncation).

## Nerve before/after (galion-nerve 8670, galion-nerve-shadow 8672)
Before at 05:03:28, after at 05:10:37 +02:00, both HTTP 200. Per component
(name, pid, status, unit_sub, n_restarts_systemd), all 15 components on both
nerves were **identical**. The `systemctl show` lines were identical too:
galion-nerve 1466797, galion-nerve-shadow 4008991, hermes-rs 1361728,
openmind-memory 1374875, tuner 4004553, tuner-quick 1198838, cloudflared
1468472, link-watch-rs 1359406, galion-desk 231, all `active running`,
NRestarts 0.

The only difference was the shadow's top-level `system.status`, which went
from `degraded` to `failing`. Its verdict at 05:10 was "request flood on
cloudflared (stream edge-cloudflared)" plus the existing closed-origin
incident. That signal comes from public traffic in cloudflared's log, which
the sandbox cannot reach (no network, S7). The runner's only host activity
was `journalctl` reads and two loopback decoy sockets. The cortex is not live
yet, so the nerve was the observer.

## Cleanup proof (after run 3, 05:10:32-05:10:34 +02:00)
```
LEFT_PROCS_BEFORE_KILL 0
TMPFS unmounted
VERIFY user: id: 'sbxtest': no such user
VERIFY group: none
VERIFY dirs: 0          (/var/tmp/sandbox-test-*)
VERIFY units: 0         (systemctl list-units --all 'sbxtest*')
VERIFY cgroups: 0       (sbxtest* / om-sbx-* under /sys/fs/cgroup)
VERIFY procs: 0
VERIFY mounts: 0
ORIG_NOW    0ff84fad0d753fd3 /var/lib/memory-tencentdb/instances/galion/vectors.db
ORIG_NOW    f50024d3b47a2837 /var/lib/memory-tencentdb/instances/galion/vectors.db-wal
```
The same checks passed after each earlier prep and run (05:05:10, 05:07:40,
05:09:30): 0 left each time. `/opt/galion/openmind` was not touched. No live
service was stopped, restarted or reconfigured. There were no DNS, HADES or
Hermes model changes. The box target dir
`/workspace/swarm/target/sandbox-realprob` was deleted.

## Reproduce
Build: `cd tools/sandbox-real-problems && CARGO_TARGET_DIR=... cargo-slot build --release --target x86_64-unknown-linux-musl -j 2`.
On the VPS, as root (prep and cleanup only):
1. Make `/var/tmp/sandbox-test-<ts>/{bin,data/db,w}` and the temp user.
2. Write the journal copy through `srp mask`, `ref.txt`, the DB copy,
   `secret-paths.txt` and the tmpfs.
3. Start the `systemd-run` line above.
4. Clean up and verify as shown.

`SRP_ONLY=S1,S7` runs a subset.

## Appendix: full run-3 output

Journal timestamps (`first=`/`last=`) converted from +00:00 to +02:00; nothing else edited.

```
run start 2026-10-21 05:10:02.099 +0200
uid=996 euid=996
CGROUP self /sys/fs/cgroup/sbxtest.slice/sbxtest-realprob-20261021-050940.service
CGROUP delegated parent /sys/fs/cgroup/sbxtest.slice/sbxtest-realprob-20261021-050940.service subtree_control=cpu memory pids
DETECT per_run_possible=true reason="per-run cgroup possible under /sbxtest.slice/sbxtest-realprob-20261021-050940.service: subtree already delegates cpu, memory, and pids"
FEATURES Features { landlock_abi: 0, userns: true, userns_full: true, mount_setattr: true, caps: true, android: false } landlock_abi=None
REFS 70 keys

=== S1 cloudflared 502 diagnosis on a masked copy of the last 24 h of journal
EXPECT tool exits 0; every count equals root's grep count on the live journal for the same window
RUN ["diag502", "/var/tmp/sandbox-test-20261021-050940/data/journal.log"] status=Exited(0) timed_out=false wall_ms=94 error=None stdout_truncated=false
LAYERS rlimits=Enforced | env=Enforced | workdir=Enforced | no_new_privs=Enforced | userns=Enforced | net=Enforced | pidns=Enforced | mountns=Enforced | ro_root=Enforced | hide_home=Enforced | proc=NotEnforced("proc mount refused (e.g. container with overmounted /proc); host /proc stays visible") | landlock=NotEnforced("kernel has no landlock") | caps=Enforced | fds=Enforced | cgroup=NotEnforced("no cgroup_parent in policy; no pids.max / memory.max for the tree")
RESULT diag502 lines=54888 err_lines=54882 requests=27439 events=27439 first=2026-10-20T05:08:47+02:00 last=2026-10-21T05:08:38+02:00 ms=81
RESULT diag502 origin http://127.0.0.1:3010 1293
RESULT diag502 origin http://127.0.0.1:7681 6
RESULT diag502 origin http://127.0.0.1:8000 1295
RESULT diag502 origin http://localhost:3004 193
RESULT diag502 origin http://localhost:3005 722
RESULT diag502 origin http://localhost:3011 7413
RESULT diag502 origin http://localhost:3015 280
RESULT diag502 origin http://localhost:3020 453
RESULT diag502 origin http://localhost:3080 551
RESULT diag502 origin http://localhost:3333 17
RESULT diag502 origin http://localhost:4000 1909
RESULT diag502 origin http://localhost:4001 7827
RESULT diag502 origin http://localhost:4003 186
RESULT diag502 origin http://localhost:4004 389
RESULT diag502 origin http://localhost:4020 3308
RESULT diag502 origin http://localhost:4042 474
RESULT diag502 origin http://localhost:4060 828
RESULT diag502 origin http://localhost:4444 291
RESULT diag502 origin http://localhost:5530 4
RESULT diag502 dest adriaticpadel.com 766
RESULT diag502 dest adriaticpadel.com:2083 1
RESULT diag502 dest adriaticpadel.com:8080 142
RESULT diag502 dest adriaticpadel.com:8443 145
RESULT diag502 dest adriaticpadel.com:8880 142
RESULT diag502 dest api.galion.app 1085
RESULT diag502 dest api.galion.app:443 140
RESULT diag502 dest api.galion.app:8443 70
RESULT diag502 dest code.galion.app 6
RESULT diag502 dest developer.galion.app 17
RESULT diag502 dest docs.galion.app 474
RESULT diag502 dest downloader.galion.app 288
RESULT diag502 dest downloader.galion.app:443 3
RESULT diag502 dest galion.app 1138
RESULT diag502 dest galion.app:443 71
RESULT diag502 dest galion.app:8443 140
RESULT diag502 dest galion.studio 6058
RESULT diag502 dest galion.studio:443 1
RESULT diag502 dest knowledge.galion.app 280
RESULT diag502 dest machine.galion.app 187
RESULT diag502 dest machine.galion.app:443 6
RESULT diag502 dest maivirtue.com 437
RESULT diag502 dest merlinelement.com 7285
RESULT diag502 dest reference.galion.app 551
RESULT diag502 dest slaviknomad.com 819
RESULT diag502 dest studio.galion.app 1203
RESULT diag502 dest tactical-belts.galion.app 389
RESULT diag502 dest torchbearer.galion.app 224
RESULT diag502 dest torchbearer.galion.app:443 254
RESULT diag502 dest torchbearer.galion.app:8443 244
RESULT diag502 dest tulu.galion.app 4
RESULT diag502 dest voice.galion.app 183
RESULT diag502 dest voice.galion.app:443 3
RESULT diag502 dest workspace3d.app 2073
RESULT diag502 dest ws3d.app 1065
RESULT diag502 dest www.adriaticpadel.com 97
RESULT diag502 dest www.galion.app 560
RESULT diag502 dest www.galion.studio 565
RESULT diag502 dest www.maivirtue.com 16
RESULT diag502 dest www.merlinelement.com 128
RESULT diag502 dest www.slaviknomad.com 9
RESULT diag502 dest www.workspace3d.app 116
RESULT diag502 dest www.ws3d.app 54
HOST OK [S1] tool exited 0 (got Exited(0))
HOST OK [S1] j.dest adriaticpadel.com: sandbox 766 vs host 766
HOST OK [S1] j.dest adriaticpadel.com:2083: sandbox 1 vs host 1
HOST OK [S1] j.dest adriaticpadel.com:8080: sandbox 142 vs host 142
HOST OK [S1] j.dest adriaticpadel.com:8443: sandbox 145 vs host 145
HOST OK [S1] j.dest adriaticpadel.com:8880: sandbox 142 vs host 142
HOST OK [S1] j.dest api.galion.app: sandbox 1085 vs host 1085
HOST OK [S1] j.dest api.galion.app:443: sandbox 140 vs host 140
HOST OK [S1] j.dest api.galion.app:8443: sandbox 70 vs host 70
HOST OK [S1] j.dest code.galion.app: sandbox 6 vs host 6
HOST OK [S1] j.dest developer.galion.app: sandbox 17 vs host 17
HOST OK [S1] j.dest docs.galion.app: sandbox 474 vs host 474
HOST OK [S1] j.dest downloader.galion.app: sandbox 288 vs host 288
HOST OK [S1] j.dest downloader.galion.app:443: sandbox 3 vs host 3
HOST OK [S1] j.dest galion.app: sandbox 1138 vs host 1138
HOST OK [S1] j.dest galion.app:443: sandbox 71 vs host 71
HOST OK [S1] j.dest galion.app:8443: sandbox 140 vs host 140
HOST OK [S1] j.dest galion.studio: sandbox 6058 vs host 6058
HOST OK [S1] j.dest galion.studio:443: sandbox 1 vs host 1
HOST OK [S1] j.dest knowledge.galion.app: sandbox 280 vs host 280
HOST OK [S1] j.dest machine.galion.app: sandbox 187 vs host 187
HOST OK [S1] j.dest machine.galion.app:443: sandbox 6 vs host 6
HOST OK [S1] j.dest maivirtue.com: sandbox 437 vs host 437
HOST OK [S1] j.dest merlinelement.com: sandbox 7285 vs host 7285
HOST OK [S1] j.dest reference.galion.app: sandbox 551 vs host 551
HOST OK [S1] j.dest slaviknomad.com: sandbox 819 vs host 819
HOST OK [S1] j.dest studio.galion.app: sandbox 1203 vs host 1203
HOST OK [S1] j.dest tactical-belts.galion.app: sandbox 389 vs host 389
HOST OK [S1] j.dest torchbearer.galion.app: sandbox 224 vs host 224
HOST OK [S1] j.dest torchbearer.galion.app:443: sandbox 254 vs host 254
HOST OK [S1] j.dest torchbearer.galion.app:8443: sandbox 244 vs host 244
HOST OK [S1] j.dest tulu.galion.app: sandbox 4 vs host 4
HOST OK [S1] j.dest voice.galion.app: sandbox 183 vs host 183
HOST OK [S1] j.dest voice.galion.app:443: sandbox 3 vs host 3
HOST OK [S1] j.dest workspace3d.app: sandbox 2073 vs host 2073
HOST OK [S1] j.dest ws3d.app: sandbox 1065 vs host 1065
HOST OK [S1] j.dest www.adriaticpadel.com: sandbox 97 vs host 97
HOST OK [S1] j.dest www.galion.app: sandbox 560 vs host 560
HOST OK [S1] j.dest www.galion.studio: sandbox 565 vs host 565
HOST OK [S1] j.dest www.maivirtue.com: sandbox 16 vs host 16
HOST OK [S1] j.dest www.merlinelement.com: sandbox 128 vs host 128
HOST OK [S1] j.dest www.slaviknomad.com: sandbox 9 vs host 9
HOST OK [S1] j.dest www.workspace3d.app: sandbox 116 vs host 116
HOST OK [S1] j.dest www.ws3d.app: sandbox 54 vs host 54
HOST OK [S1] j.events: sandbox 27439 vs host 27439
HOST OK [S1] j.lines: sandbox 54888 vs host 54888
HOST OK [S1] j.origin http://127.0.0.1:3010: sandbox 1293 vs host 1293
HOST OK [S1] j.origin http://127.0.0.1:7681: sandbox 6 vs host 6
HOST OK [S1] j.origin http://127.0.0.1:8000: sandbox 1295 vs host 1295
HOST OK [S1] j.origin http://localhost:3004: sandbox 193 vs host 193
HOST OK [S1] j.origin http://localhost:3005: sandbox 722 vs host 722
HOST OK [S1] j.origin http://localhost:3011: sandbox 7413 vs host 7413
HOST OK [S1] j.origin http://localhost:3015: sandbox 280 vs host 280
HOST OK [S1] j.origin http://localhost:3020: sandbox 453 vs host 453
HOST OK [S1] j.origin http://localhost:3080: sandbox 551 vs host 551
HOST OK [S1] j.origin http://localhost:3333: sandbox 17 vs host 17
HOST OK [S1] j.origin http://localhost:4000: sandbox 1909 vs host 1909
HOST OK [S1] j.origin http://localhost:4001: sandbox 7827 vs host 7827
HOST OK [S1] j.origin http://localhost:4003: sandbox 186 vs host 186
HOST OK [S1] j.origin http://localhost:4004: sandbox 389 vs host 389
HOST OK [S1] j.origin http://localhost:4020: sandbox 3308 vs host 3308
HOST OK [S1] j.origin http://localhost:4042: sandbox 474 vs host 474
HOST OK [S1] j.origin http://localhost:4060: sandbox 828 vs host 828
HOST OK [S1] j.origin http://localhost:4444: sandbox 291 vs host 291
HOST OK [S1] j.origin http://localhost:5530: sandbox 4 vs host 4
HOST OK [S1] j.requests: sandbox 27439 vs host 27439
HOST OK [S1] masked copy has 0 non-loopback IPv4 left (host grep)
NOTE [S1] 65 keys compared
SCENARIO S1 PASS ms=94 | 65 keys compared

=== S2 integrity check of a COPY of the openmind-memory SQLite DB (+WAL)
EXPECT copy has 0 structural errors with and without the WAL; header matches root's od reading; sandbox and unsandboxed results identical; both deliberate corruptions detected
RUN ["dbsuite", "/var/tmp/sandbox-test-20261021-050940/data/db/vectors.db", "/var/tmp/sandbox-test-20261021-050940/data/db/vectors.db-wal"] status=Exited(0) timed_out=false wall_ms=32 error=None stdout_truncated=false
RESULT dbcheck-main page_size=4096 header_pages=979 file_pages=979 pages=979 btree=874 overflow=104 freelist=1/1 ptrmap=0 orphans=0 objects=32 errors=0
RESULT dbcheck-main object table sqlite_schema root=1 entries=31
RESULT dbcheck-main object table embedding_meta root=2 entries=0
RESULT dbcheck-main object index sqlite_autoindex_embedding_meta_1 root=3 entries=0
RESULT dbcheck-main object table l1_records root=4 entries=216
RESULT dbcheck-main object index sqlite_autoindex_l1_records_1 root=5 entries=216
RESULT dbcheck-main object index idx_l1_type root=6 entries=216
RESULT dbcheck-main object index idx_l1_session_key root=7 entries=216
RESULT dbcheck-main object index idx_l1_session_id root=8 entries=216
RESULT dbcheck-main object index idx_l1_scene root=9 entries=216
RESULT dbcheck-main object index idx_l1_ts_start root=10 entries=216
RESULT dbcheck-main object index idx_l1_ts_end root=11 entries=216
RESULT dbcheck-main object index idx_l1_session_updated root=12 entries=216
RESULT dbcheck-main object index idx_l1_sessionkey_updated root=13 entries=216
RESULT dbcheck-main object table l0_conversations root=14 entries=2596
RESULT dbcheck-main object index sqlite_autoindex_l0_conversations_1 root=15 entries=2596
RESULT dbcheck-main object index idx_l0_session root=16 entries=2596
RESULT dbcheck-main object index idx_l0_session_id root=17 entries=2596
RESULT dbcheck-main object index idx_l0_recorded root=18 entries=2596
RESULT dbcheck-main object index idx_l0_timestamp root=19 entries=2596
RESULT dbcheck-main object table l1_fts root=0 entries=0
RESULT dbcheck-main object table l1_fts_data root=20 entries=18
RESULT dbcheck-main object table l1_fts_idx root=21 entries=16
RESULT dbcheck-main object table l1_fts_content root=22 entries=216
RESULT dbcheck-main object table l1_fts_docsize root=23 entries=216
RESULT dbcheck-main object table l1_fts_config root=24 entries=1
RESULT dbcheck-main object table l0_fts root=0 entries=0
RESULT dbcheck-main object table l0_fts_data root=25 entries=91
RESULT dbcheck-main object table l0_fts_idx root=26 entries=89
RESULT dbcheck-main object table l0_fts_content root=28 entries=2596
RESULT dbcheck-main object table l0_fts_docsize root=30 entries=2596
RESULT dbcheck-main object table l0_fts_config root=31 entries=1
RESULT dbcheck-main object table schema_migrations root=940 entries=1
RESULT dbcheck-wal page_size=4096 header_pages=979 file_pages=979 pages=995 btree=885 overflow=110 freelist=0/0 ptrmap=0 orphans=0 objects=32 errors=0
RESULT dbcheck-wal wal frames=1013 valid=236 commits=14 db_pages=995 distinct_pages=60 error=None
RESULT dbcheck-wal object table sqlite_schema root=1 entries=31
RESULT dbcheck-wal object table embedding_meta root=2 entries=0
RESULT dbcheck-wal object index sqlite_autoindex_embedding_meta_1 root=3 entries=0
RESULT dbcheck-wal object table l1_records root=4 entries=216
RESULT dbcheck-wal object index sqlite_autoindex_l1_records_1 root=5 entries=216
RESULT dbcheck-wal object index idx_l1_type root=6 entries=216
RESULT dbcheck-wal object index idx_l1_session_key root=7 entries=216
RESULT dbcheck-wal object index idx_l1_session_id root=8 entries=216
RESULT dbcheck-wal object index idx_l1_scene root=9 entries=216
RESULT dbcheck-wal object index idx_l1_ts_start root=10 entries=216
RESULT dbcheck-wal object index idx_l1_ts_end root=11 entries=216
RESULT dbcheck-wal object index idx_l1_session_updated root=12 entries=216
RESULT dbcheck-wal object index idx_l1_sessionkey_updated root=13 entries=216
RESULT dbcheck-wal object table l0_conversations root=14 entries=2610
RESULT dbcheck-wal object index sqlite_autoindex_l0_conversations_1 root=15 entries=2610
RESULT dbcheck-wal object index idx_l0_session root=16 entries=2610
RESULT dbcheck-wal object index idx_l0_session_id root=17 entries=2610
RESULT dbcheck-wal object index idx_l0_recorded root=18 entries=2610
RESULT dbcheck-wal object index idx_l0_timestamp root=19 entries=2610
RESULT dbcheck-wal object table l1_fts root=0 entries=0
RESULT dbcheck-wal object table l1_fts_data root=20 entries=18
RESULT dbcheck-wal object table l1_fts_idx root=21 entries=16
RESULT dbcheck-wal object table l1_fts_content root=22 entries=216
RESULT dbcheck-wal object table l1_fts_docsize root=23 entries=216
RESULT dbcheck-wal object table l1_fts_config root=24 entries=1
RESULT dbcheck-wal object table l0_fts root=0 entries=0
RESULT dbcheck-wal object table l0_fts_data root=25 entries=106
RESULT dbcheck-wal object table l0_fts_idx root=26 entries=104
RESULT dbcheck-wal object table l0_fts_content root=28 entries=2610
RESULT dbcheck-wal object table l0_fts_docsize root=30 entries=2610
RESULT dbcheck-wal object table l0_fts_config root=31 entries=1
RESULT dbcheck-wal object table schema_migrations root=940 entries=1
RESULT dbcheck ms=7
RESULT dbcorrupt-type root=30 errors=2 first=Some("page 30: bad b-tree page type 7 (expected table b-tree)")
RESULT dbcorrupt-trunc errors=2 first=Some("header says 979 pages, file has 978")
HOST OK [S2] tool exited 0 (got Exited(0))
HOST OK [S2] sandboxed and unsandboxed checker output identical (67 lines)
HOST OK [S2] dbcheck-main errors=0
HOST OK [S2] dbcheck-wal errors=0
HOST OK [S2] page_size: checker 4096 vs root od 4096
HOST OK [S2] header_pages: checker 979 vs root od 979
HOST OK [S2] copy size 4009984 vs original 4009984
HOST OK [S2] dbcorrupt-type detected (2 errors)
HOST OK [S2] dbcorrupt-trunc detected (2 errors)
SCENARIO S2 PASS ms=55

=== S3 crash-looping job under a naive restart-forever supervisor
EXPECT every child dies of SIGABRT and is restarted; the run is killed at the 4 s wall timeout; 0 processes left; per-run cgroup removed
RUN ["crashloop", "/var/tmp/sandbox-test-20261021-050940/data/journal.log"] status=Signaled(9) timed_out=true wall_ms=4007 error=None stdout_truncated=false
OUT 104 progress lines, last: CRASH 104 sig=Some(6) code=None
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=19529728 (18 MiB) pids_peak=3 removed=true
HOST OK [S3] killed at wall timeout
HOST OK [S3] wall 4007 ms
HOST OK [S3] 104 crashes observed and restarted
HOST OK [S3] 104/104 children ended with SIGABRT
HOST OK [S3] 0 processes left in the workdir
HOST OK [S3] per-run cgroup removed
SCENARIO S3 PASS ms=4309

=== S4 runaway memory: a 'cache' that keeps every journal line of every re-read
EXPECT A: memory.max 128 MiB (RLIMIT_AS 2 GiB) -> cgroup OOM kill, peak <= 128 MiB; B: no cgroup, RLIMIT_AS 128 MiB -> allocation fails, process dies on a signal
RUN ["memhog", "/var/tmp/sandbox-test-20261021-050940/data/journal.log"] status=Signaled(9) timed_out=false wall_ms=1754 error=None stdout_truncated=false
OUT 4 progress lines, last: MEM iter=3 held_mib=70
CGROUP pids_max_hits=0 oom_kills=3 mem_max_hits=12825 mem_peak=134217728 (128 MiB) pids_peak=2 removed=true
HOST OK [S4] A killed by SIGKILL, not by the timeout (status Signaled(9), timed_out false)
HOST OK [S4] A oom_kills=3
HOST OK [S4] A memory.peak 128 MiB <= 128
HOST OK [S4] A per-run cgroup removed
NOTE [S4] A last progress: MEM iter=3 held_mib=70; wall 1754 ms
RUN ["memhog", "/var/tmp/sandbox-test-20261021-050940/data/journal.log"] status=Signaled(11) timed_out=false wall_ms=235 error=None stdout_truncated=false
OUT 4 progress lines, last: MEM iter=3 held_mib=70
STDERR last: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
HOST OK [S4] B stopped by RLIMIT_AS (status Signaled(11))
NOTE [S4] B last progress: MEM iter=3 held_mib=70; wall 235 ms
HOST OK [S4] no processes left
SCENARIO S4 PASS ms=1991 | A last progress: MEM iter=3 held_mib=70; wall 1754 ms; B last progress: MEM iter=3 held_mib=70; wall 235 ms

=== S5 fork storm: parallel 502 count that spawns 200 workers at once
EXPECT pids.max 24 refuses forks (EAGAIN), pids.peak <= 24, the job falls back in-process and still reports the exact host count
RUN ["forkjob", "/var/tmp/sandbox-test-20261021-050940/data/journal.log", "200"] status=Exited(0) timed_out=false wall_ms=1031 error=None stdout_truncated=false
RESULT forkjob workers=200 spawned=22 spawn_failed=178 first_err="Resource temporarily unavailable (os error 11)" total_requests=27439 ms=1004
CGROUP pids_max_hits=178 oom_kills=0 mem_max_hits=0 mem_peak=410693632 (391 MiB) pids_peak=24 removed=true
HOST OK [S5] job exited 0 (Exited(0))
HOST OK [S5] spawned 22, refused 178
HOST OK [S5] pids.events max=178
HOST OK [S5] pids.peak=24
HOST OK [S5] job total 27439 vs host 27439
HOST OK [S5] no processes left
SCENARIO S5 PASS ms=1032

=== S6 hung task with a helper and a setsid()-detached daemon
EXPECT killed at the 3 s wall timeout (wall < 3.6 s); 3 tagged processes alive mid-run, 0 afterwards; both without and with the per-run cgroup
RUN ["hang", "hang4040690nocg"] status=Signaled(9) timed_out=true wall_ms=3002 error=None stdout_truncated=false
OUT 1 progress lines, last: HANG started helpers, now blocking
HOST OK [S6] nocg: timed_out
HOST OK [S6] nocg: wall 3002 ms
HOST OK [S6] nocg: 3 tagged processes mid-run (hang + helper + detached)
HOST OK [S6] nocg: 0 tagged processes after teardown
RUN ["hang", "hang4040690cg"] status=Signaled(9) timed_out=true wall_ms=3001 error=None stdout_truncated=false
OUT 1 progress lines, last: HANG started helpers, now blocking
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=1384448 (1 MiB) pids_peak=5 removed=true
HOST OK [S6] cg: timed_out
HOST OK [S6] cg: wall 3001 ms
HOST OK [S6] cg: 3 tagged processes mid-run (hang + helper + detached)
HOST OK [S6] cg: 0 tagged processes after teardown
SCENARIO S6 PASS ms=7008

=== S7 task reaching for live services 8670/8672/8642/8650/8420, the internet and DNS
EXPECT every connect/send/resolve fails (ENETUNREACH or resolver error); only 'lo' visible; decoy host listeners get 0 connections and 0 bytes; positive control from the host does reach the decoy
RUN ["net", "42783", "39157"] status=Exited(0) timed_out=false wall_ms=5034 error=None stdout_truncated=false
RESULT net live-8670 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8672 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8642 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8650 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-8420 CONTAINED Network unreachable (os error 101) ms=0
RESULT net live-v6-8670 CONTAINED Address not available (os error 99) ms=0
RESULT net internet-1.1.1.1:443 CONTAINED Network unreachable (os error 101) ms=0
RESULT net internet-8.8.8.8:53 CONTAINED Network unreachable (os error 101) ms=0
RESULT net decoy-tcp CONTAINED Network unreachable (os error 101) ms=0
RESULT net decoy-udp CONTAINED Network unreachable (os error 101)
RESULT net dns-udp-8.8.8.8 CONTAINED Network unreachable (os error 101)
RESULT net resolve CONTAINED failed to lookup address information: Try again
RESULT net interfaces 1 ["lo"]
HOST OK [S7] net layer Enforced
HOST OK [S7] 12 probes, 0 escaped
HOST OK [S7] only lo inside the sandbox
HOST OK [S7] decoys during sandbox run: 0 conns, 0 bytes, 0 datagrams
HOST OK [S7] positive control reached decoys: +1 conn, +7 bytes, +1 datagram
SCENARIO S7 PASS ms=6572

=== S8 task trying to read secrets: galion-light/secrets, /root, /root/galion-secrets, .env files, /etc/shadow, service /proc/<pid>/environ
EXPECT every path CONTAINED or HIDDEN (never ESCAPED) with the shipped preset AND with hide_paths emptied (unix permissions alone); third run = proposed fix (preset + hide /opt/galion) must make every path unreadable, templates included
NOTE [S8] 48 paths
RUN ["secrets", "/opt/galion/galion-light/secrets", "/root/galion-secrets", "/root", "/root/.ssh", "/root/.ssh/authorized_keys", "/etc/shadow", "/etc/openmind-memory.env", "/etc/memory-gateway.env", "/opt/galion/tooling/hermes-agent/.env.example", "/opt/galion/galion-light/hermes-home/.env", "/opt/galion/galion-light/publish/obsidian/link.env", "/opt/galion/galion-light/shadow/hermes-agent/.env.example", "/opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example", "/opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env", "/opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local", "/opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/.env.bak-20261018-193614+0200", "/opt/galion/hermes/.env", "/opt/galion/hermes/.env.sealed", "/opt/galion/galion-src/merlin-element/.env", "/opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/galion-src/magic-merlin-showcase/.env", "/opt/galion/galion-src/galion-backend/.env.backup", "/opt/galion/galion-src/galion-backend/.env", "/opt/galion/galion-src/galion-console/.env.example", "/opt/galion/galion-src/galion-rs/.env.example", "/opt/galion/galion-src/tactical-belts-pro/.env.local", "/opt/galion/galion-src/tactical-belts-pro/.env.example", "/proc/1466797/environ", "/proc/4008991/environ", "/proc/1361728/environ", "/proc/1374875/environ", "/proc/1468472/environ", "/proc/4004553/environ", "/proc/1/environ"] status=Exited(0) timed_out=false wall_ms=14 error=None stdout_truncated=false
RESULT secret /opt/galion/galion-light/secrets CONTAINED Permission denied (os error 13)
RESULT secret /root/galion-secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root HIDDEN listed 0 entries (empty cover)
RESULT secret /root/.ssh CONTAINED No such file or directory (os error 2)
RESULT secret /root/.ssh/authorized_keys CONTAINED No such file or directory (os error 2)
RESULT secret /etc/shadow CONTAINED Permission denied (os error 13)
RESULT secret /etc/openmind-memory.env CONTAINED Permission denied (os error 13)
RESULT secret /etc/memory-gateway.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/tooling/hermes-agent/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/hermes-home/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-light/publish/obsidian/link.env ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/shadow/hermes-agent/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env.bak-20261018-193614+0200 CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env.sealed CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/merlin-element/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/magic-merlin-showcase/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-backend/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-console/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/galion-rs/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /proc/1466797/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4008991/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1361728/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1374875/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1468472/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4004553/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1/environ CONTAINED Permission denied (os error 13)
NOTE [S8] preset: 10 world-readable .env.example templates readable (not secrets)
HOST OK [S8] preset: 48 of 48 paths judged
HOST FAIL [S8] preset: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]
NOTE [S8] preset: 36 contained, 1 hidden
RUN ["secrets", "/opt/galion/galion-light/secrets", "/root/galion-secrets", "/root", "/root/.ssh", "/root/.ssh/authorized_keys", "/etc/shadow", "/etc/openmind-memory.env", "/etc/memory-gateway.env", "/opt/galion/tooling/hermes-agent/.env.example", "/opt/galion/galion-light/hermes-home/.env", "/opt/galion/galion-light/publish/obsidian/link.env", "/opt/galion/galion-light/shadow/hermes-agent/.env.example", "/opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example", "/opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env", "/opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local", "/opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/.env.bak-20261018-193614+0200", "/opt/galion/hermes/.env", "/opt/galion/hermes/.env.sealed", "/opt/galion/galion-src/merlin-element/.env", "/opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/galion-src/magic-merlin-showcase/.env", "/opt/galion/galion-src/galion-backend/.env.backup", "/opt/galion/galion-src/galion-backend/.env", "/opt/galion/galion-src/galion-console/.env.example", "/opt/galion/galion-src/galion-rs/.env.example", "/opt/galion/galion-src/tactical-belts-pro/.env.local", "/opt/galion/galion-src/tactical-belts-pro/.env.example", "/proc/1466797/environ", "/proc/4008991/environ", "/proc/1361728/environ", "/proc/1374875/environ", "/proc/1468472/environ", "/proc/4004553/environ", "/proc/1/environ"] status=Exited(0) timed_out=false wall_ms=14 error=None stdout_truncated=false
RESULT secret /opt/galion/galion-light/secrets CONTAINED Permission denied (os error 13)
RESULT secret /root/galion-secrets CONTAINED Permission denied (os error 13)
RESULT secret /root CONTAINED Permission denied (os error 13)
RESULT secret /root/.ssh CONTAINED Permission denied (os error 13)
RESULT secret /root/.ssh/authorized_keys CONTAINED Permission denied (os error 13)
RESULT secret /etc/shadow CONTAINED Permission denied (os error 13)
RESULT secret /etc/openmind-memory.env CONTAINED Permission denied (os error 13)
RESULT secret /etc/memory-gateway.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/tooling/hermes-agent/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/hermes-home/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-light/publish/obsidian/link.env ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/shadow/hermes-agent/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env.bak-20261018-193614+0200 CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/hermes/.env.sealed CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/merlin-element/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/magic-merlin-showcase/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-backend/.env.backup CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-backend/.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/galion-console/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/galion-rs/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.local CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.example ESCAPED readable (1 byte read, not shown)
RESULT secret /proc/1466797/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4008991/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1361728/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1374875/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1468472/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4004553/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1/environ CONTAINED Permission denied (os error 13)
NOTE [S8] nohide: 10 world-readable .env.example templates readable (not secrets)
HOST OK [S8] nohide: 48 of 48 paths judged
HOST FAIL [S8] nohide: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]
NOTE [S8] nohide: 37 contained, 0 hidden
RUN ["secrets", "/opt/galion/galion-light/secrets", "/root/galion-secrets", "/root", "/root/.ssh", "/root/.ssh/authorized_keys", "/etc/shadow", "/etc/openmind-memory.env", "/etc/memory-gateway.env", "/opt/galion/tooling/hermes-agent/.env.example", "/opt/galion/galion-light/hermes-home/.env", "/opt/galion/galion-light/publish/obsidian/link.env", "/opt/galion/galion-light/shadow/hermes-agent/.env.example", "/opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example", "/opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local", "/opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example", "/opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env", "/opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env", "/opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local", "/opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup", "/opt/galion/hermes/.env.bak-20261018-193614+0200", "/opt/galion/hermes/.env", "/opt/galion/hermes/.env.sealed", "/opt/galion/galion-src/merlin-element/.env", "/opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example", "/opt/galion/galion-src/magic-merlin-showcase/.env", "/opt/galion/galion-src/galion-backend/.env.backup", "/opt/galion/galion-src/galion-backend/.env", "/opt/galion/galion-src/galion-console/.env.example", "/opt/galion/galion-src/galion-rs/.env.example", "/opt/galion/galion-src/tactical-belts-pro/.env.local", "/opt/galion/galion-src/tactical-belts-pro/.env.example", "/proc/1466797/environ", "/proc/4008991/environ", "/proc/1361728/environ", "/proc/1374875/environ", "/proc/1468472/environ", "/proc/4004553/environ", "/proc/1/environ"] status=Exited(0) timed_out=false wall_ms=14 error=None stdout_truncated=false
RESULT secret /opt/galion/galion-light/secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root/galion-secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root HIDDEN listed 0 entries (empty cover)
RESULT secret /root/.ssh CONTAINED No such file or directory (os error 2)
RESULT secret /root/.ssh/authorized_keys CONTAINED No such file or directory (os error 2)
RESULT secret /etc/shadow CONTAINED Permission denied (os error 13)
RESULT secret /etc/openmind-memory.env CONTAINED Permission denied (os error 13)
RESULT secret /etc/memory-gateway.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/tooling/hermes-agent/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/hermes-home/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/publish/obsidian/link.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/shadow/hermes-agent/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env.bak-20261018-193614+0200 CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env.sealed CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/merlin-element/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/magic-merlin-showcase/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-backend/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /proc/1466797/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4008991/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1361728/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1374875/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1468472/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4004553/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1/environ CONTAINED Permission denied (os error 13)
NOTE [S8] preset+hide-opt-galion: 0 world-readable .env.example templates readable (not secrets)
HOST OK [S8] preset+hide-opt-galion: 48 of 48 paths judged
HOST OK [S8] preset+hide-opt-galion: 0 non-template ESCAPED []
HOST OK [S8] preset+hide-opt-galion: 0 readable at all (templates included)
NOTE [S8] preset+hide-opt-galion: 47 contained, 1 hidden
NOTE [S8] proposed fix (hide /opt/galion in tool presets) closes every readable path
SCENARIO S8 FAIL ms=48 failed: preset: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]; nohide: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"] | 48 paths; preset: 10 world-readable .env.example templates readable (not secrets); preset: 36 contained, 1 hidden; nohide: 10 world-readable .env.example templates readable (not secrets); nohide: 37 contained, 0 hidden; preset+hide-opt-galion: 0 world-readable .env.example templates readable (not secrets); preset+hide-opt-galion: 47 contained, 1 hidden; proposed fix (hide /opt/galion in tool presets) closes every readable path

=== S9 disk filling the workdir
EXPECT a: one 64 MiB file under RLIMIT_FSIZE 16 MiB -> SIGXFSZ kill (or EFBIG when SIGXFSZ is ignored), file stops at exactly 16 MiB; b: 32 x 8 MiB files -> all written (no total quota in openmind-sandbox: GAP); c: same on a 32 MiB tmpfs workdir -> ENOSPC at ~32 MiB
RUN ["fill", "64", "1"] status=Exited(0) timed_out=false wall_ms=12 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=0 total_mib=16 err=File too large (os error 27)
OUT 4 progress lines, last: W 16
HOST OK [S9] a: status Exited(0), EFBIG seen true (SIGXFSZ kill or EFBIG both stop it)
HOST OK [S9] a: file size 16777216 bytes
NOTE [S9] b: free on the workdir fs before 76674 MiB
RUN ["fill", "8", "32"] status=Exited(0) timed_out=false wall_ms=184 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill done files=32 total_mib=256
OUT 64 progress lines, last: W 256
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=1925120 (1 MiB) pids_peak=2 removed=true
HOST OK [S9] b: status Exited(0)
NOTE [S9] b: 256 MiB written in 185 ms, no quota stopped it
RUN ["fill", "8", "10"] status=Exited(0) timed_out=false wall_ms=23 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=3 total_mib=31 err=No space left on device (os error 28)
OUT 7 progress lines, last: W 28
HOST OK [S9] c: ENOSPC on the 32 MiB tmpfs
NOTE [S9] c: stopped at 31 MiB
NOTE [S9] GAP: openmind-sandbox has only RLIMIT_FSIZE (per file); total workdir size is unbounded unless the caller gives it a size-limited fs
SCENARIO S9 PASS+GAP ms=243 | b: free on the workdir fs before 76674 MiB; b: 256 MiB written in 185 ms, no quota stopped it; c: stopped at 31 MiB; GAP: openmind-sandbox has only RLIMIT_FSIZE (per file); total workdir size is unbounded unless the caller gives it a size-limited fs

=== S10 the known /proc gap: what a sandboxed tool can see of the host
EXPECT (known gap, measured) host process list and cmdlines visible, same as the bare temp user sees; other users' environ and exe links NOT readable
RUN ["procview"] status=Exited(0) timed_out=false wall_ms=31 error=None stdout_truncated=false
LAYERS rlimits=Enforced | env=Enforced | workdir=Enforced | no_new_privs=Enforced | userns=Enforced | net=Enforced | pidns=Enforced | mountns=Enforced | ro_root=Enforced | hide_home=Enforced | proc=NotEnforced("proc mount refused (e.g. container with overmounted /proc); host /proc stays visible") | landlock=NotEnforced("kernel has no landlock") | caps=Enforced | fds=Enforced | cgroup=NotEnforced("no cgroup_parent in policy; no pids.max / memory.max for the tree")
RESULT procview pids=49 other_uid=46 comm_readable=49 distinct_names=37 cmdline_readable=49 other_uid_cmdline_readable=46 environ_readable=1 other_uid_environ_readable=0 exe_link_readable=1 cmdlines_with_secretish_words=0
RESULT procview services_identifiable cloudflared=true galion-nerve=true galion-nerve-sh=true hermes-gateway=true openmind-memory=true sshd=true systemd=true tuner=true
RESULT procview file /proc/meminfo readable lines=55
RESULT procview file /proc/loadavg readable lines=1
RESULT procview file /proc/uptime readable lines=1
RESULT procview file /proc/cpuinfo readable lines=216
RESULT procview file /proc/net/tcp readable lines=1
RESULT procview file /proc/mounts readable lines=45
RESULT procview file /proc/1/cmdline readable lines=1
RESULT procview file /proc/1/environ denied Permission denied (os error 13)
BARE RESULT procview pids=48 other_uid=46 comm_readable=48 distinct_names=36 cmdline_readable=48 other_uid_cmdline_readable=46 environ_readable=2 other_uid_environ_readable=0 exe_link_readable=2 cmdlines_with_secretish_words=0
HOST OK [S10] tool exited 0
NOTE [S10] sandbox sees 49 pids, bare user 48, root counted 45
HOST OK [S10] other-uid environ readable: 0
NOTE [S10] other-uid cmdlines readable in sandbox: 46 (GAP confirmed)
HOST OK [S10] sandbox view == bare-user view (+-10 churn)
SCENARIO S10 MEASURED (known gap) ms=51 | sandbox sees 49 pids, bare user 48, root counted 45; other-uid cmdlines readable in sandbox: 46 (GAP confirmed)

=== SUMMARY total_ms=21422
SCENARIO S1 PASS ms=94 | 65 keys compared
SCENARIO S2 PASS ms=55
SCENARIO S3 PASS ms=4309
SCENARIO S4 PASS ms=1991 | A last progress: MEM iter=3 held_mib=70; wall 1754 ms; B last progress: MEM iter=3 held_mib=70; wall 235 ms
SCENARIO S5 PASS ms=1032
SCENARIO S6 PASS ms=7008
SCENARIO S7 PASS ms=6572
SCENARIO S8 FAIL ms=48 failed: preset: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]; nohide: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"] | 48 paths; preset: 10 world-readable .env.example templates readable (not secrets); preset: 36 contained, 1 hidden; nohide: 10 world-readable .env.example templates readable (not secrets); nohide: 37 contained, 0 hidden; preset+hide-opt-galion: 0 world-readable .env.example templates readable (not secrets); preset+hide-opt-galion: 47 contained, 1 hidden; proposed fix (hide /opt/galion in tool presets) closes every readable path
SCENARIO S9 PASS+GAP ms=243 | b: free on the workdir fs before 76674 MiB; b: 256 MiB written in 185 ms, no quota stopped it; c: stopped at 31 MiB; GAP: openmind-sandbox has only RLIMIT_FSIZE (per file); total workdir size is unbounded unless the caller gives it a size-limited fs
SCENARIO S10 MEASURED (known gap) ms=51 | sandbox sees 49 pids, bare user 48, root counted 45; other-uid cmdlines readable in sandbox: 46 (GAP confirmed)
SUMMARY 10 scenarios, 1 FAIL
run exit 1 end 2026-10-21 05:10:23 +0200
```

## Appendix: run-3 prep output (root, names/modes only)

```
TS=20261021-050940 BASE=/var/tmp/sandbox-test-20261021-050940 start 2026-10-21 05:09:40 +0200
37ec242b63f107cf30907118e4a3b10fe3f4cffd168e4fdc030a7f004c76cd84  /var/tmp/sandbox-test-20261021-050940/bin/srp
uid=996(sbxtest) gid=989(sbxtest) groups=989(sbxtest)
WINDOW 2026-10-20 05:08:40 +0200 .. 2026-10-21 05:08:40 +0200
MASK replaced v4=27467 v6=0 bytes=18428925
ORIG_BEFORE 0ff84fad0d753fd3 /var/lib/memory-tencentdb/instances/galion/vectors.db
ORIG_BEFORE f50024d3b47a2837 /var/lib/memory-tencentdb/instances/galion/vectors.db-wal
ORIG_AFTER  0ff84fad0d753fd3 /var/lib/memory-tencentdb/instances/galion/vectors.db
ORIG_AFTER  f50024d3b47a2837 /var/lib/memory-tencentdb/instances/galion/vectors.db-wal
COPY        0ff84fad0d753fd3 /var/tmp/sandbox-test-20261021-050940/data/db/vectors.db
COPY        f50024d3b47a2837 /var/tmp/sandbox-test-20261021-050940/data/db/vectors.db-wal
SECRET_PATHS 48
PERM 700 root:root directory /opt/galion/galion-light/secrets
PERM 700 root:root directory /root/galion-secrets
PERM 700 root:root directory /root
PERM 700 root:root directory /root/.ssh
PERM 600 root:root file /root/.ssh/authorized_keys
PERM 640 root:shadow file /etc/shadow
PERM 600 root:root file /etc/openmind-memory.env
PERM 600 root:root file /etc/memory-gateway.env
PERM 666 root:root file /opt/galion/tooling/hermes-agent/.env.example
PERM 600 root:root file /opt/galion/galion-light/hermes-home/.env
PERM 666 root:root file /opt/galion/galion-light/publish/obsidian/link.env
PERM 666 root:root file /opt/galion/galion-light/shadow/hermes-agent/.env.example
PERM 666 root:root file /opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example
PERM 600 root:root file /opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env
PERM 600 root:root file /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local
PERM 644 root:root file /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example
PERM 644 root:root file /opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example
PERM 755 root:root file /opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example
PERM 755 root:root file /opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example
PERM 600 root:root file /opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env
PERM 600 root:root file /opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env
PERM 700 root:root file /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env
PERM 700 root:root file /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup
PERM 755 root:root file /opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example
PERM 755 root:root file /opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example
PERM 644 root:root file /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example
PERM 600 root:root file /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local
PERM 644 root:root file /opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example
PERM 700 root:root file /opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup
PERM 600 root:root file /opt/galion/hermes/.env.bak-20261018-193614+0200
PERM 600 root:root file /opt/galion/hermes/.env
PERM 600 root:root file /opt/galion/hermes/.env.sealed
PERM 600 root:root file /opt/galion/galion-src/merlin-element/.env
PERM 666 root:root file /opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example
PERM 600 root:root file /opt/galion/galion-src/magic-merlin-showcase/.env
PERM 700 root:root file /opt/galion/galion-src/galion-backend/.env.backup
PERM 700 root:root file /opt/galion/galion-src/galion-backend/.env
PERM 777 root:root file /opt/galion/galion-src/galion-console/.env.example
PERM 777 root:root file /opt/galion/galion-src/galion-rs/.env.example
PERM 600 root:root file /opt/galion/galion-src/tactical-belts-pro/.env.local
PERM 666 root:root file /opt/galion/galion-src/tactical-belts-pro/.env.example
PERM 400 root:root regular empty file /proc/1466797/environ
PERM 400 root:root regular empty file /proc/4008991/environ
PERM 400 root:root regular empty file /proc/1361728/environ
PERM 400 root:root regular empty file /proc/1374875/environ
PERM 400 root:root regular empty file /proc/1468472/environ
PERM 400 root:root regular empty file /proc/4004553/environ
PERM 400 root:root regular empty file /proc/1/environ
PROCMOUNT proc proc rw,nosuid,nodev,noexec,relatime
TMPFS mounted tmpfs  tmpfs  rw,nosuid,nodev,relatime,size=32768k,mode=755,uid=100996,gid=100989,inode64
REF:
j.lines	54888
j.requests	27439
j.events	27439
j.origin http://127.0.0.1:3010	1293
j.origin http://127.0.0.1:7681	6
j.origin http://127.0.0.1:8000	1295
j.origin http://localhost:3004	193
j.origin http://localhost:3005	722
j.origin http://localhost:3011	7413
j.origin http://localhost:3015	280
j.origin http://localhost:3020	453
j.origin http://localhost:3080	551
j.origin http://localhost:3333	17
j.origin http://localhost:4000	1909
j.origin http://localhost:4001	7827
j.origin http://localhost:4003	186
j.origin http://localhost:4004	389
j.origin http://localhost:4020	3308
j.origin http://localhost:4042	474
j.origin http://localhost:4060	828
j.origin http://localhost:4444	291
j.origin http://localhost:5530	4
j.dest adriaticpadel.com	766
j.dest adriaticpadel.com:2083	1
j.dest adriaticpadel.com:8080	142
j.dest adriaticpadel.com:8443	145
j.dest adriaticpadel.com:8880	142
j.dest api.galion.app	1085
j.dest api.galion.app:443	140
j.dest api.galion.app:8443	70
j.dest code.galion.app	6
j.dest developer.galion.app	17
j.dest docs.galion.app	474
j.dest downloader.galion.app	288
j.dest downloader.galion.app:443	3
j.dest galion.app	1138
j.dest galion.app:443	71
j.dest galion.app:8443	140
j.dest galion.studio	6058
j.dest galion.studio:443	1
j.dest knowledge.galion.app	280
j.dest machine.galion.app	187
j.dest machine.galion.app:443	6
j.dest maivirtue.com	437
j.dest merlinelement.com	7285
j.dest reference.galion.app	551
j.dest slaviknomad.com	819
j.dest studio.galion.app	1203
j.dest tactical-belts.galion.app	389
j.dest torchbearer.galion.app	224
j.dest torchbearer.galion.app:443	254
j.dest torchbearer.galion.app:8443	244
j.dest tulu.galion.app	4
j.dest voice.galion.app	183
j.dest voice.galion.app:443	3
j.dest workspace3d.app	2073
j.dest ws3d.app	1065
j.dest www.adriaticpadel.com	97
j.dest www.galion.app	560
j.dest www.galion.studio	565
j.dest www.maivirtue.com	16
j.dest www.merlinelement.com	128
j.dest www.slaviknomad.com	9
j.dest www.workspace3d.app	116
j.dest www.ws3d.app	54
j.masked_ipv4_left	0
db.page_size	4096
db.header_pages	979
db.file_bytes	4009984
proc.host_pids	45
prep done 2026-10-21 05:09:59 +0200
```
