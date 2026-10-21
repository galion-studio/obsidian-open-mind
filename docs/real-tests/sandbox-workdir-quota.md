<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# sandbox-workdir-quota: real-life VPS proof (wave-4 slot 151)

Branch `swarm/sandbox-workdir-quota`, based on `swarm/sandbox-real-problems`
334c633. Design: `docs/sandbox/workdir-quota.md`. All times Europe/Zagreb,
CEST +02:00. Sandbox runs only on the VPS (sandbox_vps_only); nothing
sandboxed ran on the box.

## Box (no sandbox runs)
- `cargo fmt --check` OK (root workspace and tools/sandbox-real-problems).
- `cargo clippy --workspace --all-targets -- -D warnings` OK; runner clippy
  (musl release, all targets) OK.
- `cargo test -p openmind-sandbox --lib`: 5 passed (treecopy: copy in, copy out
  only changes and never follows a host symlink, byte/entry budget incl. a
  1 GiB sparse file -> EDQUOT, depth -> ELOOP and FIFO skipped, 500 entries
  across several getdents batches). Plain file copies in a temp dir, no
  sandbox.
- `cargo test -p openmind-sandbox --test preset`: 2 passed (pure policy).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  957 passed, 0 failed, 3 ignored (205 test binaries).
- tools/sandbox-real-problems unit tests: 3 passed.
- hermes-tools: built (clippy, musl test build); its tests were not run
  (they run the sandbox, so not on the box; on the VPS `tests/sandboxed.rs`
  skips itself because the kernel has no Landlock). Its tools set the cap to 0.

## VPS run (2026-10-21 08:03:14 - 08:11:35 +02:00)
- Throwaway dir `/var/tmp/sandbox-test-20261021-080314` (`bin/` root 0755;
  `data/`, `w/` owned by the temp user). Temp user `sbx151` (system, nologin,
  uid 996). Every run: `systemd-run --wait --pipe --collect --slice=sbxtest.slice -p User=sbx151 -p Group=sbx151 -p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10 ...`.
- Kernel 6.8.12-43-pve (LXC). `FEATURES landlock_abi: 0, userns: true,
  mount_setattr: true`. Per-run cgroup: unit cgroup delegated
  (`subtree_control=cpu memory pids`).
- Data for S1-S10 prepared by root exactly as in sandbox-real-problems run 3:
  cloudflared journal 2026-10-20 08:03:30 .. 2026-10-21 08:03:30 +02:00,
  masked by `srp mask` running as sbx151 (55,916 lines, 0 non-loopback IPv4
  left by host grep); `ref.txt` 72 keys from root's grep/awk/od/stat; copy of
  the openmind-memory SQLite DB + WAL (sha256 prefixes `0ff84fad0d753fd3`,
  `f50024d3b47a2837` before the copy, after the copy, of the copies, and after
  all runs: unchanged, originals never opened for writing); 48 secret paths
  (the 41 files of run 3 + `/proc/<MainPID>/environ` of 6 services + pid 1;
  only the word ESCAPED/CONTAINED is printed, no content); a 32 MiB host tmpfs
  on `w/s9c-tmpfs` (S9c/c2).
- Binaries (static x86_64 musl, built on the box from this branch):
  `srp` 881,512 B sha256 `132477acb1aa874a...`, `real_workdir_quota`
  987,936 B `4be745ec400ebffa...`, `sandbox` suite 1,004,320 B
  `cc0a0aa7da93a77a...`.

### Bug found and fixed by the real run
Run 1 (08:03:43-08:04:05) with the interrupted agent's code: the cap held
(S9b stopped at 63 MiB of 256, S9d at 255 MiB of 320) but **every copy back
failed with errno 30 (EROFS)**: S9a/b/c2/d got 0 bytes back, S2's scratch file
too. Cause: `ro_root` remounts the whole tree read-only and the host work dir
mount sits under the tmpfs, out of reach of the re-enabled write bind. Fix: the
host dir fd now comes from a detached `open_tree(OPEN_TREE_CLONE|AT_RECURSIVE)`
clone, outside the tree. Run 2 (08:05:49) S9 PASS. Run-1 q-test output also
showed 3 test-script bugs (q7 `tail -1` hid the ENOSPC line, q8 `: >` is a
special builtin whose redirection error exits dash, q9 needed a delegated
parent); fixed, and q9 now asserts the OOM instead of passing vacuously.

### Final run (08:10:27-08:10:58), binaries above
| Suite | Result |
|---|---|
| srp S1-S10 | 8 PASS (S1-S7, S9), S10 MEASURED (known /proc gap, unchanged), **S8 FAIL = the known 334c633 link.env finding**, not caused by this row: this branch is based on 334c633, the fix is `swarm/sandbox-hide-galion` ad0ed9b (slot 150; the runner's own `preset+hide-opt-galion` baseline line shows 0 readable). `SUMMARY 10 scenarios, 1 FAIL` |
| real_workdir_quota (GALION_REAL=1, q1-q9) | **9 passed, 0 failed** (3.93 s) |
| sandbox suite (tests/sandbox.rs, cap off path) | **26 passed, 0 failed** (5.13 s) |

Layers with the shipped preset: `workdir_quota=Enforced`, all others as in
334c633 (`proc` and `landlock` NotEnforced on this kernel, unchanged).

S9 (shipped read-only-tool preset unless noted):
| Case | Asked | Result |
|---|---|---|
| a: RLIMIT_FSIZE 16 MiB | 64 MiB file | EFBIG at exactly 16,777,216 B, file copied back (16 MiB on the host) |
| b: preset cap 64 MiB | 32 x 8 MiB = 256 MiB | ENOSPC, tool stopped at 63 MiB, host got 66,224,128 B (63 MiB), copy back complete, 121 ms. **S9b gap closed** (334c633: 256 MiB in 184 ms) |
| c: cap off, 32 MiB host tmpfs | 80 MiB | ENOSPC at 31 MiB (old path), layer reported NotEnforced |
| c2: cap 64 MiB over that 32 MiB host fs | 80 MiB | tool stopped at the 64 MiB cap (63 MiB), copy back partial: 31 MiB, errno 28, `out_complete=false` |
| d: cap 256 MiB (row 151 proof) | 320 MiB | **ENOSPC at 255 MiB**, tmpfs used 268,435,456 B, host got 267,550,720 B, complete, 411 ms, memory.peak 258 MiB |
| e: cap 256 MiB, memory.max 128 MiB | 320 MiB | Signaled(9), 3 OOM kills, last progress 124 MiB, nothing copied back, host 0 MiB |

real_workdir_quota (read-only-tool preset unless noted):
| Test | Result |
|---|---|
| q1 cap 256 MiB, `dd` 320 MiB | dd "No space left on device", rc=1; tmpfs used 268,435,456 B; host file 268,435,456 B (= cap), out_complete |
| q2 shipped 64 MiB, 32 x 8 MiB | stop=8; host 67,108,864 B |
| q3 inputs in, changes back | in 5 entries/200,033 B; out.txt/sub/state/new/deep/x back; deleted input.txt still on host; keep.bin mtime unchanged (2 unchanged) |
| q4 3 MiB work dir, 2 MiB cap | `Status::Error` "work dir tmpfs: copying the work dir in failed (No space left on device ...)"; command never ran; input untouched |
| q5 wall timeout 2 s | timed out, copied_out=false, nothing on the host |
| q6 900 MiB sparse file | tmpfs used 4096 B; copy back refused, errno 122 (EDQUOT); nothing on the host |
| q7 escape attempts | fds in the tool 0,1,2 (+ ls's own 3); `umount` and `umount -l` of the work dir: "must be superuser" rc=32 (caps dropped); 100 MiB dd stops at 64 MiB; host 67,108,864 B |
| q8 inode cap 1024 | 1023 files then ENOSPC; host 1023 entries; bare `SandboxPolicy::new` -> `workdir_quota` None + NotEnforced |
| q9 cap 256 MiB, memory.max 128 MiB | Signaled(9), 4 OOM kills, 18 memory.max hits, mem_peak 134,217,728 B, per-run cgroup removed, host 0 B |

### Live system during the runs (read-only)
- Nerve `127.0.0.1:8670/state` at 08:11: 17 healthy, 2 degrading
  (galion-one-mouth HTTP 503 since 2026-10-18 22:35, cloudflared origin errors
  since 2026-10-21 05:35, both before these runs), 3 absent (not deployed),
  restarts_1h 0, no alerts. openmind-memory, hermes-rs, cloudflared,
  galion-nerve active. The cortex is not deployed; the nerve is the observer.
- `/opt/galion/openmind` still at 2f5a730, not touched. link.env still 666
  (owner, unchanged). No service, unit, DNS, HADES or Hermes model change.

### Cleanup (08:11:35)
Processes of sbx151: 0; host tmpfs unmounted; 0 mounts left; dir removed;
user and group sbx151 removed; 0 `sbx151*` units; 0 sbx151 cgroups under
sbxtest.slice; no `/var/tmp/sandbox-test-*` left. Box target dir removed after
the push.

## Appendix: final-run S9 output
```
HOST OK [S8] preset+hide-opt-galion: 0 readable at all (templates included)
NOTE [S8] preset+hide-opt-galion: 47 contained, 1 hidden
NOTE [S8] proposed fix (hide /opt/galion in tool presets) closes every readable path
SCENARIO S8 FAIL ms=41 failed: preset: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"]; nohide: 1 non-template ESCAPED ["/opt/galion/galion-light/publish/obsidian/link.env"] | 48 paths; preset: 10 world-readable .env.example templates readable (not secrets); preset: 36 contained, 1 hidden; nohide: 10 world-readable .env.example templates readable (not secrets); nohide: 37 contained, 0 hidden; preset+hide-opt-galion: 0 world-readable .env.example templates readable (not secrets); preset+hide-opt-galion: 47 contained, 1 hidden; proposed fix (hide /opt/galion in tool presets) closes every readable path

=== S9 disk filling the workdir
EXPECT a: one 64 MiB file under RLIMIT_FSIZE 16 MiB -> EFBIG at exactly 16 MiB, file copied back; b: 32 x 8 MiB = 256 MiB under the shipped preset (64 MiB work dir tmpfs) -> ENOSPC at the 64 MiB cap, host work dir gets <= 64 MiB back (S9b gap closed); c: cap off on a 32 MiB host tmpfs -> ENOSPC at ~32 MiB (old path), c2: 64 MiB cap over that 32 MiB host fs -> tool stops at the cap, copy back reports ENOSPC; d: 256 MiB cap, 320 MiB write -> ENOSPC at the 256 MiB limit, <= 256 MiB copied back; e: 256 MiB cap under a 128 MiB memory cgroup -> tmpfs pages are charged, measured
RUN ["fill", "64", "1"] status=Exited(0) timed_out=false wall_ms=33 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=0 total_mib=16 err=File too large (os error 27)
OUT 4 progress lines, last: W 16
WDQ limit=64 MiB inodes=4096 in=881512B/1 entries used=17661952 B (16 MiB) copied_out=true complete=true out=16777216B/1 entries unchanged=1 skipped=0 errno=0
HOST OK [S9] a: status Exited(0), EFBIG seen true (SIGXFSZ kill or EFBIG both stop it)
HOST OK [S9] a: file size 16777216 bytes (copied back)
NOTE [S9] b: free on the workdir fs before 76665 MiB
HOST OK [S9] b: shipped preset cap 64 MiB
RUN ["fill", "8", "32"] status=Exited(0) timed_out=false wall_ms=121 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=7 total_mib=63 err=No space left on device (os error 28)
OUT 15 progress lines, last: W 60
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=68861952 (65 MiB) pids_peak=2 removed=true
WDQ limit=64 MiB inodes=4096 in=881512B/1 entries used=67108864 B (64 MiB) copied_out=true complete=true out=66224128B/8 entries unchanged=1 skipped=0 errno=0
HOST OK [S9] b: status Exited(0)
HOST OK [S9] b: ENOSPC inside the sandbox
HOST OK [S9] b: tool stopped at 63 MiB of 256 (cap 64)
HOST OK [S9] b: host work dir got 63 MiB back (66224128 B)
HOST OK [S9] b: workdir_quota enforced, copy back complete (66224128 B, 8 entries, errno 0)
NOTE [S9] b: 256 MiB asked, 63 MiB written, 63 MiB on the host, 121 ms (gap closed)
RUN ["fill", "8", "10"] status=Exited(0) timed_out=false wall_ms=32 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=3 total_mib=31 err=No space left on device (os error 28)
OUT 7 progress lines, last: W 28
HOST OK [S9] c: cap off: ENOSPC on the 32 MiB host tmpfs
HOST OK [S9] c: cap off is reported NotEnforced
NOTE [S9] c: stopped at 31 MiB
RUN ["fill", "8", "10"] status=Exited(0) timed_out=false wall_ms=94 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=7 total_mib=63 err=No space left on device (os error 28)
OUT 15 progress lines, last: W 60
WDQ limit=64 MiB inodes=4096 in=881512B/1 entries used=67108864 B (64 MiB) copied_out=true complete=false out=32669696B/5 entries unchanged=0 skipped=0 errno=28
HOST OK [S9] c2: tool stopped at the 64 MiB cap (63 MiB)
HOST OK [S9] c2: copy back hit the full host fs: complete=false errno=28
NOTE [S9] c2: host fs smaller than the cap: 31 MiB of 63 MiB came back, reported as errno 28 (a caller must check out_complete)
RUN ["fill", "8", "40"] status=Exited(0) timed_out=false wall_ms=411 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
RESULT fill stop file=31 total_mib=255 err=No space left on device (os error 28)
OUT 63 progress lines, last: W 252
CGROUP pids_max_hits=0 oom_kills=0 mem_max_hits=0 mem_peak=270606336 (258 MiB) pids_peak=2 removed=true
WDQ limit=256 MiB inodes=16384 in=881512B/1 entries used=268435456 B (256 MiB) copied_out=true complete=true out=267550720B/32 entries unchanged=1 skipped=0 errno=0
HOST OK [S9] d: ENOSPC, status Exited(0)
HOST OK [S9] d: stopped at 255 MiB of 320 asked (cap 256)
HOST OK [S9] d: tmpfs used 268435456 B at the end
HOST OK [S9] d: 255 MiB copied back (267550720 B), complete true
NOTE [S9] d: memory.peak 258 MiB (tmpfs pages are charged to the run cgroup)
NOTE [S9] d: 320 MiB asked, ENOSPC at 255 MiB, 255 MiB on the host, 411 ms
RUN ["fill", "8", "40"] status=Signaled(9) timed_out=false wall_ms=813 error=None stdout_truncated=false
RESULT fill sigxfsz ignored=false blocked=false pid_in_ns=1
OUT 31 progress lines, last: W 124
CGROUP pids_max_hits=0 oom_kills=3 mem_max_hits=18 mem_peak=134217728 (128 MiB) pids_peak=2 removed=true
WDQ limit=256 MiB inodes=16384 in=0B/0 entries used=0 B (0 MiB) copied_out=false complete=false out=0B/0 entries unchanged=0 skipped=0 errno=0
HOST OK [S9] e: host got 0 MiB back (<= memory.max 128 MiB)
NOTE [S9] e: cap 256 MiB under memory.max 128 MiB: status Signaled(9), oom_kills 3, stop -, last progress 'W 124', copy back Some((false, false, 0)), host 0 MiB
SCENARIO S9 PASS ms=1538 | b: free on the workdir fs before 76665 MiB; b: 256 MiB asked, 63 MiB written, 63 MiB on the host, 121 ms (gap closed); c: stopped at 31 MiB; c2: host fs smaller than the cap: 31 MiB of 63 MiB came back, reported as errno 28 (a caller must check out_complete); d: memory.peak 258 MiB (tmpfs pages are charged to the run cgroup); d: 320 MiB asked, ENOSPC at 255 MiB, 255 MiB on the host, 411 ms; e: cap 256 MiB under memory.max 128 MiB: status Signaled(9), oom_kills 3, stop -, last progress 'W 124', copy back Some((false, false, 0)), host 0 MiB

=== S10 the known /proc gap: what a sandboxed tool can see of the host
EXPECT (known gap, measured) host process list and cmdlines visible, same as the bare temp user sees; other users' environ and exe links NOT readable
```

## Appendix: final-run real_workdir_quota output
```
test q1_256_mib_cap_write_320_enospc_at_limit ... Running as unit: sbx151-wdq-q3-20261021-080314.service; invocation ID: 1d9a03464dd040908578dd35a6a64d55
RESULT q1 asked=320MiB host_bytes=268435456 used=268435456
test q2_shipped_preset_caps_many_files_at_64_mib ... --- i=0; while [ $i -lt 32 ]; do dd if=/dev/zero of=f$i bs=1M count=8 2>/dev/null || { echo stop=$i; break; }; i=$((i+1)); done; echo done=$i
RESULT q2 asked=256MiB host_bytes=67108864
test q3_inputs_copied_in_changes_copied_back_deletions_not ... --- wc -l < link > out.txt && echo v2 > sub/state && mkdir -p new/deep && echo hi > new/deep/x && rm input.txt && ls -A | sort | tr '\n' ' '
RESULT q3 WorkdirQuota { limit_bytes: 67108864, limit_inodes: 4096, in_bytes: 200033, in_entries: 5, in_skipped: 0, used_bytes: 212992, copied_out: true, out_complete: true, out_bytes: 8, out_entries: 5, out_unchanged: 2, out_skipped: 0, errno: 0 }
test q4_workdir_bigger_than_cap_fails_closed ... --- touch ran-anyway
RESULT q4 error=work dir tmpfs: copying the work dir in failed (No space left on device (os error 28); 2097152 bytes, 1 entries copied); a work dir larger than workdir_tmpfs_bytes=2097152 / nr_inodes=1024 cannot run under this cap
test q5_timeout_copies_nothing_back ... --- echo partial > half-done; sleep 30
RESULT q5 WorkdirQuota { limit_bytes: 67108864, limit_inodes: 4096, in_bytes: 0, in_entries: 0, in_skipped: 0, used_bytes: 0, copied_out: false, out_complete: false, out_bytes: 0, out_entries: 0, out_unchanged: 0, out_skipped: 0, errno: 0 }
test q6_sparse_file_never_becomes_host_disk ... ok
test q7_no_host_fd_no_umount_cap_holds ... --- dd if=/dev/zero of=sparse bs=1 count=1 seek=943718399 2>&1; ls -ls sparse; df -k . | tail -1
RESULT q6 WorkdirQuota { limit_bytes: 67108864, limit_inodes: 4096, in_bytes: 0, in_entries: 0, in_skipped: 0, used_bytes: 4096, copied_out: true, out_complete: false, out_bytes: 0, out_entries: 0, out_unchanged: 0, out_skipped: 0, errno: 122 } host_exists=false
RESULT q7 host_bytes=67108864
test q8_inode_cap_and_off_switch ... --- i=0; while [ $i -lt 2000 ]; do true > n$i 2>/dev/null || { echo stop=$i; break; }; i=$((i+1)); done
RESULT q8 stop=1023 host_entries=1023
test q9_cap_bigger_than_memory_cgroup ... --- echo ok
CGROUP parent /sys/fs/cgroup/sbxtest.slice/sbx151-wdq-q3-20261021-080314.service subtree_control=cpu memory pids
RESULT q9 status=Signaled(9) cgroup=Some(CgroupStats { path: "/sys/fs/cgroup/sbxtest.slice/sbx151-wdq-q3-20261021-080314.service/om-sbx-4192872-0", pids_max_hits: 0, oom_kills: 4, mem_max_hits: 18, mem_peak: 134217728, pids_peak: 3, removed: true }) quota=Some(WorkdirQuota { limit_bytes: 268435456, limit_inodes: 16384, in_bytes: 0, in_entries: 0, in_skipped: 0, used_bytes: 0, copied_out: false, out_complete: false, out_bytes: 0, out_entries: 0, out_unchanged: 0, out_skipped: 0, errno: 0 }) host_bytes=0
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.93s
```
