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

## hermes-tools
`crates/hermes-tools/src/exec.rs` sets `workdir_tmpfs_bytes = 0` for its
coding tools: their work dir is a whole git worktree (sources, `.git`,
`target/`), so the copy in/out would cost a full tree copy per tool call and a
worktree above the cap would fail closed. They keep their per-file caps and
the cgroup. A worktree-level quota for Hermes is owner decision D-WDQ2.

## Owner decisions
- D-WDQ1: preset caps (64 MiB / 64 MiB / 512 MiB) OK, or other numbers?
- D-WDQ2: Hermes coding tools: keep the cap off (now), or a cap with a
  bigger size and the copy cost accepted, or a different mechanism (e.g. a
  project quota / per-worktree loop fs prepared by root)?
- D-WDQ3: partial copy back on host ENOSPC leaves partial files; keep
  (reported via `out_complete`) or roll back what was written?
