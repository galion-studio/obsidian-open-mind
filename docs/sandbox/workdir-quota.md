<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-sandbox: total work dir size cap (`workdir_tmpfs_bytes`)

Wave-4 slot 151 (sandbox-workdir-quota). Closes the S9b gap found by
`swarm/sandbox-real-problems` 334c633 (a sandboxed tool wrote 256 MiB into
its work dir in 184 ms; only `RLIMIT_FSIZE`, a per-file limit, existed).
Times are Europe/Zagreb, CEST +02:00.

## How it works
- `SandboxPolicy::workdir_tmpfs_bytes` (0 = off, the `SandboxPolicy::new`
  default) and `workdir_tmpfs_inodes` (0 = bytes / 16 KiB, at least 1024).
- Needs the mount and PID namespaces (no host root): after `/` is made
  private and `hide_paths` are mounted, the sandbox
  1. opens the host work dir as a directory fd. With `ro_root` this fd comes
     from a detached recursive clone of its mount (`open_tree(OPEN_TREE_CLONE |
     AT_RECURSIVE)`), so the later read-only remount of the tree does not reach
     it (first VPS run 08:03: every copy back failed with EROFS without this);
  2. mounts `tmpfs size=<bytes>,nr_inodes=<n>,mode=700,nosuid,nodev` over the
     work dir;
  3. copies the host work dir in (regular files, dirs, symlinks; mode without
     set-uid/set-gid, mtime; FIFOs/sockets/devices skipped and counted). A
     work dir that does not fit fails the run closed (`Status::Error`, the
     command never starts);
  4. then applies `ro_root` (the write bind of the work dir now binds the
     tmpfs), `RLIMIT_FSIZE`, the PID-namespace fork, caps drop and so on.
- The PID-namespace supervisor waits for PID 1. When it is gone every process
  of the namespace is gone, so nothing writes the tmpfs any more. It then
  copies back every new or changed entry (same size and mtime = unchanged,
  left alone), within a byte budget = the tmpfs size and an entry budget =
  the inode cap, so a sparse file cannot become more host disk than the cap
  (EDQUOT). Host entries of another type (e.g. a symlink where the tmpfs has a
  dir) are never replaced and never followed; every name is resolved with
  `O_NOFOLLOW` relative to a dir fd.
- The supervisor writes one fixed 88-byte report into a pipe;
  `Outcome::workdir_quota` (`WorkdirQuota`) carries it: limit, bytes/entries
  in, bytes used at the end (statvfs), `copied_out`, `out_complete`, bytes and
  entries out, unchanged, skipped, errno. Layer `workdir_quota` reports
  Enforced / NotEnforced with the reason.
- Code: `crates/openmind-sandbox/src/lib.rs` (mount + supervisor), 
  `src/treecopy.rs` (async-signal-safe tree copy: raw syscalls, no
  allocation, fixed buffers, depth limit 48 -> ELOOP).

## Semantics a caller must know
- Deletions inside the sandbox are not propagated to the host.
- Wall-timeout kill: the supervisor is killed too, nothing is copied back
  (`copied_out = false`), the host work dir is unchanged.
- Copy back can be partial: if the host fs fills up (VPS S9c2: 31 of 63 MiB
  came back, errno 28), or the budget is hit (EDQUOT). Partial files are left
  on the host. Check `out_complete`.
- tmpfs pages are memory. Under a per-run cgroup they are charged to it
  (VPS S9d: memory.peak 258 MiB for a full 256 MiB tmpfs). A tmpfs cap above
  the cgroup's memory.max turns "disk full" into an OOM kill of the whole run,
  supervisor included, and nothing is copied back (VPS S9e and q9: 4 OOM kills,
  mem_peak exactly 128 MiB, 0 bytes reached the host). Keep
  `workdir_tmpfs_bytes` well below `cgroup_mem_bytes`. Without a cgroup it is
  bounded by its size only (RLIMIT_AS does not count tmpfs pages).
- The copy in costs time and memory proportional to the work dir. A work dir
  larger than the cap cannot run under it.

## Shipped presets
| Preset | `workdir_tmpfs_bytes` | inode cap | `fsize_bytes` | cgroup mem |
|---|---|---|---|---|
| read-only-tool | 64 MiB | 4096 | 1 MiB | 256 MiB |
| net-tool | 64 MiB | 4096 | 32 MiB | 256 MiB |
| build-tool | 512 MiB | 32768 | 512 MiB | 1 GiB |

build-tool was 1 GiB in the interrupted first attempt (07:15); lowered to
512 MiB after the q9/S9e measurement so that a full work dir plus the compiler
still fit in the 1 GiB run cgroup. `deploy/sandbox/presets/*.toml` carry the
same numbers (checked by `tests/preset.rs`).

## hermes-tools (D-WDQ2, accepted 2026-10-21: cap ON)
Branch `swarm/hermes-tools-quota` (slot 151 follow-up) turns the cap on for
Hermes's coding tools. `crates/hermes-tools/src/exec.rs` sets
`workdir_tmpfs_bytes = HERMES_RS_CODING_WORKDIR_MB` MiB (default **512 MiB**,
`0`/`off` = no cap, else 16..=4096) and `workdir_tmpfs_inodes =
HERMES_RS_CODING_WORKDIR_INODES` (0 = one per 16 KiB, 32768 at 512 MiB) for
all three kinds of run.
- Why 512 MiB: the cargo tool runs on `Preset::build_tool()`, the coding
  preset, whose cap is 512 MiB (D-WDQ1). fs_read/fs_write/git run on
  `Preset::read_only_tool()` (64 MiB), but all three work on the same
  worktree and a worktree bigger than the cap fails every run closed, so a
  64 MiB cap for fs/git would refuse reads and commits on a worktree that
  cargo can build. One cap per worktree, the coding preset's number
  (`tests/workdir_quota.rs` checks it stays equal to the build-tool preset).
  `CARGO_TARGET_DIR` is outside the worktree (an `fs_write` root), so build
  output does not count toward the cap (and is not capped by it).
- Memory: tmpfs pages are charged to the per-run cgroup, so the cap is added
  on top of each kind's own `memory.max` (fs 128 + 512, git 256 + 512, cargo
  2048 + 512 MiB): a full worktree ends in ENOSPC, never in an OOM kill.
  RLIMIT_AS is unchanged (tmpfs pages are not address space).
- Startup (`preflight`) refuses when the cap is on but user namespaces are
  not available, and when the worktree (files rounded to 4 KiB pages,
  entries) is already over the cap; the startup note prints the cap and the
  current worktree size.
- Cost: every tool call copies the worktree into the tmpfs and the changes
  back (a git commit is 4-5 runs). Measured on the VPS in
  `docs/real-tests/hermes-tools-quota.md`.
- git runs with `core.checkStat=minimal`, `core.trustctime=false` (fresh
  inodes/ctimes on every copy would otherwise make each call rewrite the
  index) and `gc.auto=0` (a gc deletes loose objects, and deletions are not
  copied back).
- Hermes has no delete tool, so "deletions are not propagated" does not
  change tool behaviour; git's lock-file renames end as a changed file.

## Owner decisions (accepted 2026-10-21 08:23 +02:00, Slavik via Mate)
- D-WDQ1: preset caps 64 MiB / 64 MiB / 512 MiB — **accepted 2026-10-21**,
  unchanged.
- D-WDQ2: Hermes coding tools get the cap — **accepted 2026-10-21**,
  implemented on `swarm/hermes-tools-quota` (512 MiB, see above).
- D-WDQ3: partial copy back on host ENOSPC — **accepted 2026-10-21**: slot
  151's default is kept: partial files stay on the host (no rollback), the
  run reports `out_complete = false` + errno. Rationale: a rollback needs
  the old content kept somewhere on the same full disk, and the copy back
  only touches new/changed entries. Callers must check it: hermes-tools now
  treats an incomplete copy back as a failed tool call (`exec::ok` is false)
  and tells the model what reached the host ("worktree copy back is
  INCOMPLETE: ... partial files may be left there").
