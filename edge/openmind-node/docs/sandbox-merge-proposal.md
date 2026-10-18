<!-- SPDX-License-Identifier: MIT -->
# Sandbox merge proposal: `omnode` sandbox ↔ `crates/openmind-sandbox`

| Field | Value |
|---|---|
| Written | 2026-10-18 21:05 CEST (+02:00), Europe/Zagreb |
| Status | **Proposal only. Do not merge yet.** Hermes owns `crates/openmind-sandbox`; this document is for the boss session. |
| Project | Obsidian Open Mind, made by Galion |

## 1. What exists today

| | `edge/openmind-node` sandbox (this crate) | `crates/openmind-sandbox` (Hermes / B27) |
|---|---|---|
| Shape | Built into the `omnode` binary; CLI `sandbox run` / `check` | Library crate `openmind-sandbox`, `run(&[String], &SandboxPolicy) -> Outcome` |
| Deps | `libc`, `landlock` (crate), in-tree `seccomp` module (raw BPF) | `libc` only (Landlock via raw syscalls, ABI v1 fs rights) |
| Workdir | Fresh temp dir mode 700, removed after; HOME=TMPDIR=workdir | Caller supplies `workdir`; not created/removed by the crate |
| Env | Cleared; PATH, HOME, TMPDIR, LANG, TERM, OMNODE_SANDBOX | Cleared; allowlist (default PATH, LANG) copied from parent |
| Rlimits | CPU, AS, FSIZE, NOFILE, CORE=0 | CPU, AS, NOFILE, CORE=0 (no FSIZE) |
| Wall timeout | kill process group (`setpgid` + `kill(-pgid)`) | `setsid` + `kill(-pid)` (same idea) |
| Network | Empty netns via unprivileged user+net ns (fallback: Landlock TCP deny ABI≥4; else honest "NOT isolated") | `net: bool` reported, **not enforced** in v0 |
| Filesystem | Landlock ABI 4 (read system dirs, RW workdir only) when kernel has it | Landlock ABI 1 fs rights when kernel has it; prepared in parent, applied after fork |
| Seccomp | Deny-list (ptrace, mount, module load, bpf, keyctl, …); foreign ABI → kill | None |
| Namespaces | user + net + mount (MS_PRIVATE). **PID deferred** (see §3) | None |
| Report | JSON `Isolation` block: every layer true/false + notes | `enforced: Vec<(name, Enforced\|NotEnforced(reason))>` |
| Tests | 14 sandbox tests + 3 seccomp tests (limits, landlock, group kill, netns, seccomp) | Spec'd in `docs/agentic-sandbox.md` B27; library tests TBD by Hermes |
| Platforms | Linux + Termux (graceful fallback, notes say so) | Linux only (doc) |

## 2. Why merge

Slavik wants one consumer-grade agent sandbox for the stack. Two implementations will drift (different Landlock ABIs, different reports, different defaults). Agents and Hermes tool runners should call one API.

## 3. Known gap: PID namespaces

`omnode` tried unprivileged `CLONE_NEWPID` with a double-fork (grandchild = pid 1). It broke wall-timeout kills: after `NEWUSER`, signaling the process group from the parent left the pid1 grandchild orphaned and `wait` hung. Reproduced 2026-10-18 on the build box. **NEWPID is deferred** until a small supervisor design (pipe the grandchild's host pid back, kill by host pid, or use a tiny pid1 reaper that forwards signals). Mount + net + user namespaces stay.

## 4. Proposed end state

1. **Library** at `edge/openmind-sandbox/` (or keep the Hermes path and have omnode depend on it — Hermes decides). Public API close to B27's `SandboxPolicy` / `Outcome`, plus omnode's extras as optional fields (`fsize`, `seccomp: bool`, `namespaces: bool`).
2. **omnode** becomes a thin CLI over that library (`sandbox run` / `check` / used by `doctor`).
3. **Single Landlock path**: prefer the `landlock` crate (ABI 4, net deny) with raw-syscall fallback when the crate's ABI probe fails — or standardize on raw ABI 4. Pick one in the merge PR.
4. **Seccomp**: move `edge/openmind-node/src/seccomp.rs` into the library (it has no omnode deps).
5. **Report**: keep omnode's honest JSON shape; map it to B27's `enforced` list 1:1 so both CLIs print the same facts.
6. **Do not** edit `crates/openmind-sandbox` from the phone-instance job. Hermes owns that tree. The merge PR is a Hermes boss task.

## 5. Suggested merge steps (Hermes)

1. Freeze B27 API; add `fsize`, `seccomp`, `namespaces` fields with defaults matching omnode.
2. Port seccomp module + net/mount/user ns enter from omnode; keep NEWPID behind a feature flag off-by-default.
3. Point omnode at the library; delete the duplicated `sandbox.rs` body (keep CLI).
4. One test suite (omnode's 14 + B27's checklist from `docs/agentic-sandbox.md`).
5. Update `docs/phone-instance.md` and `docs/agentic-sandbox.md` in the same PR.

## 6. What omnode has now that B27 does not (yet)

- Empty network namespace (real `net: false`)
- Mount namespace (MS_PRIVATE)
- Seccomp deny-list
- FSIZE rlimit
- Temp workdir lifecycle
- CLI + `doctor` probe
- Termux-aware notes

## 7. What B27 has that omnode should adopt

- Clean library API (`SandboxPolicy` / `Outcome` / `Enforcement`)
- `PR_SET_NO_NEW_PRIVS` before Landlock (omnode gets this via seccomp install; should set it even when seccomp is off)
- Landlock prepared fully in the parent (omnode uses the `landlock` crate's `restrict_self` after fork — fine, but unify)
- Explicit `env_allowlist` instead of a fixed set

## 8. Decision needed from Slavik / Hermes

- Where the merged crate lives: `edge/` (standalone, like omnode) or `crates/` (workspace member).
- Whether NEWPID work is in scope for the merge or a follow-up.
