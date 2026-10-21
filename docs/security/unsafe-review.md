<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Unsafe review: openmind-node (edge) and openmind-sandbox (slot 229, S2.3)

Branch `swarm/sec-unsafe-safety`, base integration-5 `0d68f46`.
Reviewed 2026-10-21 23:47 to 2026-10-22 00:05 CEST (+02:00).

## What was done

- Every `unsafe` block in `edge/openmind-node` (43 blocks + 1 `unsafe fn`)
  and `crates/openmind-sandbox` (43 blocks: src 30, tests 3, examples 10)
  now has a `// SAFETY:` comment directly on or above it that says why the
  call is sound. Before: 31 undocumented in the edge node (sandbox.rs 17,
  seccomp.rs 14) and 1 misplaced comment in openmind-sandbox (lib.rs, the
  status-pipe read).
- The same rule found 6 more outside the two crates; documented too:
  galion-backup (statvfs/passwd/group `zeroed`), galion-guard
  `tests/real_netns.rs` (2 x `getppid`), galion-voice-cpu (mmap note, F4).
- New repo-check check `safety` (warn by default): see below.
- Code behaviour is unchanged, with one test-only fix (F2).

## Tooling: `repo-check --only safety`

`crates/repo-check/src/safety.rs`, std only. It blanks out strings, char
literals and comments, finds the `unsafe` keyword, and classifies what comes
next: a block, `fn` (not the `fn(` pointer type), `impl`, `trait`, or an
`extern` block (attributes like `#[unsafe(no_mangle)]` are not flagged).
It accepts the same places as clippy's `undocumented_unsafe_blocks` /
`missing_safety_doc`:
the same line, the comment and attribute lines directly above, or above the
start of the statement when `unsafe` sits on a continuation line. For an
`unsafe fn`, a `# Safety` doc section also counts. The scope is our own Rust
under crates/, edge/, sim/, gaia/, apps/ and tests/; vendor/ and contrib/
are left out. A line with `repo-check: allow` is skipped.

Cross-check against clippy (rust 1.99) on the base tree:

| crate | clippy undocumented_unsafe_blocks | repo-check safety |
|---|---|---|
| edge/openmind-node (all targets) | 32 | 31 (*) |
| openmind-sandbox (all targets) | 1 | 1 |

(*) The only difference: clippy also flags `// SAFETY (all probes): ...`,
because it wants the exact `SAFETY:` form. repo-check accepts the word
SAFETY in any form. The comment was rewritten to the `SAFETY:` form, so both
tools now report 0. After this branch: clippy `-D warnings -W
clippy::undocumented_unsafe_blocks -W clippy::missing_safety_doc` passes on
openmind-node, openmind-sandbox, galion-backup, galion-guard,
galion-voice-cpu and repo-check, and `repo-check --only safety` reports 0
findings across the whole tree (1521 files).

## Findings

| id | where | kind | status |
|---|---|---|---|
| F1 | edge `sandbox.rs` `run()`: `kill(-pid, SIGKILL)` after the child was reaped | logic hazard (not memory) | note only, behaviour unchanged |
| F2 | edge `seccomp.rs` tests `install_in_child_denies_ptrace`, `install_still_allows_write` | async-signal-safety after fork | **fixed (test only)** |
| F3 | edge `seccomp.rs` `install()`: `prog.len() as u16` | silent truncation if the program ever grew | regression test added |
| F4 | galion-voice-cpu `kitten/mod.rs` mmap of safetensors | relies on the file not being changed while mapped | documented |
| F5 | edge `sandbox.rs` pre_exec closure locks a `Mutex` and calls `landlock::RulesetCreated::restrict_self` in the child | reviewed: sound | documented |
| F6 | edge `util::fork_probe` contract: the child must not unwind | reviewed: sound for every current caller | documented (already) |

**F1. pgid reuse after reap (edge only).** Once `try_wait`/`wait` has
reaped the child, `run()` calls `kill(-pid, SIGKILL)` again. If every
process in that group has already exited and the kernel hands the pid to a
new process that also becomes a group leader, the signal hits an unrelated
group. The window is tiny, and the earlier timeout kill runs while the child
is still alive, so that one is fine. `openmind-sandbox` already does this
correctly: it waits with `waitid(WEXITED|WNOHANG|WNOWAIT)`, kills the group
while the child is an unreaped zombie (its pid, and so the pgid, can't be
reused yet), and only then reaps. Recommended fix (owner D-229-2): port that
pattern to the edge node. It wasn't changed here because the slot must keep
behaviour unchanged, and a sandbox behaviour change has to be tested on the
VPS.

**F2. Allocation after fork in the test harness (fixed).** Both tests
called `Filter::new()` inside the forked child, and that allocates a
`Vec<SockFilter>`. The libtest harness runs tests on several threads. If
another thread holds the allocator lock at the moment of fork, the child
deadlocks, and the test hangs instead of failing (the same class of bug as
the old sandbox flakes). The production paths (`available`, `self_test`,
`sandbox::run`) already build the filter before the fork. Fix: build the
filter before `fork()` in both tests. The assertions are identical.

**F3. u16 length.** The kernel takes `sock_fprog.len` as u16 and refuses
programs longer than BPF_MAXINSNS (4096). The new test
`program_length_fits_sock_fprog` pins `2 < len <= 4096` and that the cast is
lossless for the architecture being built. It only fails if the deny list
grows out of bounds.

**F4. mmap.** `VarBuilder::from_mmaped_safetensors` is unsafe because a
change to the file while it is mapped is undefined behaviour. This is the
usual candle pattern and an operational assumption: model files are
read-only deploy artefacts. Now stated in the SAFETY comment.

**F5. pre_exec closure (edge).** Everything the closure uses is built before
the fork: rlimit values, uid/gid map strings, the Landlock ruleset and the
seccomp program. It calls only setpgid, setrlimit, unshare, open, write,
close, mount and prctl, plus `restrict_self`. landlock 0.4.7's
`restrict_self` makes two prctl/syscalls on the success path and builds an
`io::Error` from an errno on failure, so it doesn't allocate. The `Mutex`
around the ruleset is owned only by the closure, so no thread can hold it
across the fork. Sound.

**F6.** `fork_probe` already documents its `# Safety` contract (no
allocation, no locks, no unwinding, leave through `_exit`). Every caller
(doctor userns probe, seccomp `available`/`self_test`) passes a closure of
raw syscalls plus `std::io::Error::last_os_error()`, which doesn't allocate.

## Owner decisions

- **D-229-1**: make `safety` an error in repo-check (CI fails on new
  undocumented unsafe), or add `#![warn(clippy::undocumented_unsafe_blocks)]`
  to openmind-node and openmind-sandbox (CI already runs clippy with
  `-D warnings`). Both trees are clean now, so either flip is free today.
  Proposed: the clippy lint on the two sandbox crates now, and the repo-check
  error after one wave.
- **D-229-2**: port openmind-sandbox's WNOWAIT kill-then-reap order to the
  edge sandbox (F1). Needs a VPS sandbox run (sandbox_vps_only).
- **D-229-3**: whether `SAFETY` without the colon should count. repo-check
  accepts it; clippy doesn't. Proposed: keep it lenient in repo-check and
  let clippy be the strict gate.
