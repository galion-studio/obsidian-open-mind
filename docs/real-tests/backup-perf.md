<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: galion-backup performance, bash vs Rust (wave 4, slot 119)

All times Europe/Zagreb, CEST (+02:00). Branch `swarm/backup-perf`, based on
`swarm/rust-ports` 26a2a8e. Box: 8 vCPU (avx2, sha_ni), Debian 13, ext4,
load average ~0.9 during the runs, other swarm agents active. Nothing written
on the VPS.

## Short answer

On a fake tree sized like the VPS (46 MB of SQLite, 1845 files.tar members),
**Rust `galion-backup run` uses 15-25 % more wall time and 18-31 % more CPU
than the bash script at the same gzip level 6**, and **less memory** (one
5.8-7.4 MiB process instead of 14-19 processes peaking at 10.2 MiB together).
Almost all of the CPU in both is deflate: GNU gzip -6 is faster than
miniz_oxide level 6 on the same input. Absolute cost on the VPS: about
+0.5-0.8 CPU-seconds per run, 4 runs a day, at Nice=19 / idle I/O.

With the new optional `GALION_BACKUP_GZIP_LEVEL=5`, Rust uses **36 % less
CPU than bash** (1.64 s vs 2.55 s) for a **6 % larger** archive
(19.0 MB vs 17.9 MB). Default stays 6 (owner decision B1 below).

## 1. VPS read-only probes (2026-10-21 03:35 +02:00)

ssh with `journalctl -u galion-backup`, `systemctl show`, `stat`, `find`,
`du`, `stat -f` only (names, sizes and counts; no file contents).

| What | VPS value |
|---|---|
| Bash run time (last 7 timer runs) | ~3 s (start -> "Finished"), CPU 2.36-2.62 s per run ("Consumed ... CPU time") |
| Peak memory | not recorded (`MemoryPeak=[not set]`, systemd 255) |
| hermes `state.db` | 40 779 776 B |
| memory `instances/galion/vectors.db` | 4 009 984 B + live `-wal` 4 173 592 B |
| `rs-state.db` | 913 408 B |
| memory root `vectors.db` | 126 976 B (`-wal` 0 B) |
| `/var/lib/memory-tencentdb` | 49 dirs, 1779 files (1773 non-DB: 1721 .md, 29 .jsonl, 17 .mmd, 3 .json, 1 .mdu), non-DB sum 6 275 896 B, sizes p50 737 B / p90 7734 B / p99 34 907 B |
| Archive size | 18 585 138 B (02:21:06 run), files.tar 1845 members |
| Filesystem of DEST and sources | **zfs** |

Finding (both implementations): the `size_kb=` in the VPS log is **1**, 515
or 2564 for ~18.5 MB archives. Both bash (`du -k`) and Rust (`st_blocks`)
report allocated blocks, and ZFS has not allocated them yet right after
the write. The number is meaningless on this host (decision B2).

## 2. The test (GALION_REAL=1)

`crates/galion-backup/tests/real_perf.rs`. It builds the fake tree with the VPS
numbers above (deterministic PRNG; text DB rows and notes, f32-blob vector
rows, the WAL kept live by holding a connection open like the memory
gateway does), then runs the oracle `galion-backup.sh` (git e43d7ab, sha256
checked = VPS copy) and `galion-backup run` in the same `unshare -rm` +
chroot jail as `real_parity.rs`. One warm-up each, then 5 alternating pairs.
Per run: wall time, user/sys CPU and the largest process's peak RSS from
`wait4` rusage, and the summed RSS of the live process tree sampled from
/proc every 5 ms. Then it compares the last two archives at full scale and
sweeps the Rust gzip level.

```
GALION_REAL=1 [GALION_BACKUP_BIN=<musl build>] \
  cargo test --release -p galion-backup --test real_perf -- --nocapture
```

Fake tree as measured by the test: hermes state.db 40 812 544 B, vectors.db
4 014 080 B + wal 4 173 592 B, rs-state.db 909 312 B, root vectors.db
126 976 B, memory-tencentdb 14 623 312 B; files.tar 7 792 640 B with
**1845 members**. The bash archive is **17 939 228 B** (VPS 18 585 138 B), and
bash takes **3.46 s wall / 2.55 s CPU** (VPS ~3 s / 2.5 s), so the tree is a
good stand-in.

## 3. Results (2026-10-21 03:47-03:51 +02:00), median [min-max] of 5

### Rust x86_64-gnu release (this branch)

```
wall s         bash    3.461 [3.382-3.626]   rust    3.972 [3.910-4.236]   rust/bash 1.15
user s         bash    2.327 [2.282-2.487]   rust    2.838 [2.788-2.904]   rust/bash 1.22
sys s          bash    0.222 [0.194-0.256]   rust    0.143 [0.140-0.180]   rust/bash 0.64
cpu s          bash    2.549 [2.476-2.744]   rust    2.998 [2.927-3.044]   rust/bash 1.18
maxrss MiB     bash    7.328 [7.238-7.418]   rust    7.352 [7.281-7.395]   rust/bash 1.00
tree-peak MiB  bash   10.191 [10.074-10.371]   rust    7.336 [7.281-7.379]   rust/bash 0.72
gz size bash 17939228 B rust 17956017 B; files.tar 7792640 B, 1845 members (VPS: 1845)
PASS files.tar byte-identical at VPS scale
PASS SHA256SUMS byte-identical at VPS scale
PASS files.tar member count = VPS (1845)
PASS rust peak RSS < 64 MiB
level 1: wall 1.817 s cpu 0.865 s (median) gz 21973749 B   PASS files.tar identical to bash's
level 3: wall 2.273 s cpu 1.329 s (median) gz 19187425 B   PASS
level 5: wall 2.592 s cpu 1.643 s (median) gz 19007553 B   PASS
level 6: wall 3.901 s cpu 2.951 s (median) gz 17956018 B   PASS
level 10: exit Some(1) stderr "galion-backup: GALION_BACKUP_GZIP_LEVEL must be 0-9, got \"10\""
PASS GALION_BACKUP_GZIP_LEVEL=10 refused (exit 1, DEST not created)
RESULT: 0 failed
```

### Rust x86_64-musl static release (what would be deployed)

```
wall s         bash    3.445 [3.418-3.522]   rust    4.296 [4.242-4.458]   rust/bash 1.25
user s         bash    2.370 [2.320-2.421]   rust    3.160 [3.101-3.271]   rust/bash 1.33
sys s          bash    0.188 [0.168-0.203]   rust    0.176 [0.168-0.229]   rust/bash 0.93
cpu s          bash    2.544 [2.517-2.624]   rust    3.335 [3.282-3.500]   rust/bash 1.31
maxrss MiB     bash    5.793 [5.707-5.797]   rust    5.828 [5.660-5.859]   rust/bash 1.01
tree-peak MiB  bash   10.012 [9.781-10.199]   rust    5.828 [5.660-5.859]   rust/bash 0.58
level 1: cpu 1.309 s  21973756 B | level 3: cpu 1.676 s 19187345 B
level 5: cpu 1.922 s  19007535 B | level 6: cpu 3.287 s 17955988 B
all 9 checks PASS, RESULT: 0 failed
```

(In this run the bash script's helper is the musl binary too, as on the VPS.)

### Rust 26a2a8e (before this branch), gnu, same tree, same minutes

```
cpu s          bash    2.486 [2.447-2.653]   rust    3.047 [2.979-3.111]   rust/bash 1.23
wall s         bash    3.391 [3.365-3.551]   rust    3.988 [3.947-4.056]   rust/bash 1.18
```

All content checks PASS. The old binary ignores `GALION_BACKUP_GZIP_LEVEL`, so
its sweep shows 4 x level 6 and the level-10 check fails, as expected.

## 4. Where the time goes (microbenchmarks on the 66 MB uncompressed outer tar)

| Step | Time |
|---|---|
| GNU gzip 1.13 `-6` (what bash runs) | 2.52-2.55 s user |
| miniz_oxide level 6 (Rust GzWriter) | 2.74-2.89 s |
| miniz_oxide level 5 / 3 / 1 | 1.22 / 0.95-0.98 / 0.32 s |
| coreutils `sha256sum` (uses SHA-NI) | 0.04 s |
| own pure-Rust SHA-256 (no SIMD by rule) | 0.23-0.27 s |
| own CRC-32, bytewise table (26a2a8e) | 0.155 s |
| own CRC-32, slicing-by-8 (this branch) | **0.034 s** (gnu), 0.039 s (musl) |
| `snapshot` of 40.8 MB state.db | 1.05 s wall but only ~0.1 s CPU: the backup API sleeps 20 ms after every 256 pages (~40 steps), same in bash and Rust |
| `verify` (integrity_check) of it | 0.04 s |

So bash's ~0.9 s of wall above its CPU, and Rust's, is mostly the snapshot
pauses, which are there so live writers are not blocked. Rust's extra CPU at
level 6 is miniz_oxide vs GNU gzip (~0.3 s) plus pure-Rust SHA-256 vs SHA-NI
(~0.2 s). The CRC was a third cause (~0.12 s) and is fixed here.

The musl build is ~0.3 s CPU slower than the gnu build inside `run`, but the
same microbenchmarks (deflate, SHA-256, CRC) and the stand-alone
`snapshot`/`verify` cost the same on musl and gnu. The gap is somewhere
else in the in-process run (likely allocator or many-small-read/write
behaviour with musl's mallocng). Not pinned down. Next step: phase timers.

## 5. What changed on this branch

1. `gzip.rs`: CRC-32 is now slicing-by-8 (std only, scalar, `const fn`
   tables). Same values: a new unit test checks it against a bit-at-a-time
   reference over 9 alignments x 13 lengths and split updates. Parity is
   unchanged (real_parity 26/26 again, 2026-10-21 03:44 +02:00).
2. `backup.rs`: optional `GALION_BACKUP_GZIP_LEVEL` (0-9, default 6, empty =
   default, anything else is refused up front with exit 1 before DEST is
   touched, like `GALION_BACKUP_KEEP`). It only changes the .tar.gz
   compression. files.tar, SHA256SUMS and the DB snapshots stay byte-identical
   to bash at every level (checked above).
3. `tests/real_perf.rs`: the test above.

## 6. Owner decisions

* **B1 gzip level for the deployed unit**: (a) keep 6 (default, smallest,
  +18-31 % CPU vs bash today); (b) `Environment=GALION_BACKUP_GZIP_LEVEL=5`
  in the unit: -36 % CPU vs bash (-44 % vs Rust at 6), +6 % archive (~+1 MB
  per archive, ~+13 MB over the 12 kept local + 12 offsite copies);
  (c) 3: about the same as 5 on this data. Recommendation: (b), but either
  is fine; at Nice=19 / idle I/O, 4 runs a day, CPU is not scarce
  (VPS 8 vCPU, ~1.7 GB of 16 GB RAM used).
* **B2 `size_kb` on ZFS**: keep allocated-block semantics (bash parity,
  reads ~1 KB on ZFS right after the write) vs report the apparent size
  (`st_size`, matches what the offsite hook logs). Changing it breaks
  byte-for-byte log parity with bash. Not changed here.
* Not proposed: a pure-Rust SIMD/asm SHA-256 or deflate (against the
  no-asm/SIMD rule), or a shorter snapshot pause (it protects live writers).
