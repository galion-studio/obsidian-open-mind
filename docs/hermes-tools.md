<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-tools: sandboxed coding tools for hermes-rs

Branch `swarm/hermes-tools` (slot 17), written 2026-10-20 (Europe/Zagreb, +02:00).

Until now hermes-rs had only `clock` and `delegate_task`, so it could not run a boss ticket.
This crate adds four tools. Every process they start runs inside `openmind-sandbox`.

| tool | does | refuses |
|---|---|---|
| `fs_read` | `{path, offset?, max_bytes?}`: reads one file in the worktree (`dd ... iflag=nofollow`, 256 KiB cap) | absolute paths outside the worktree, `..`, any symlink on the path, `.git` anywhere, non-regular files, unknown keys |
| `fs_write` | `{path, content}`: stages the content in a hermes-owned dir, then `install -D -m 0644` *inside the sandbox* (512 KiB cap) | same as read, plus writes under `.cargo`, `.github`, `.gitlab-ci.yml`, `.gitmodules` |
| `git` | `{op: status\|diff\|commit, paths?, staged?, message?}` on the one allowlisted branch; commit = `add -A -- <paths>` + `commit --no-verify -F <staged msg file>` | every other op (push, fetch, remote, checkout, reset, config, ...), a HEAD that is not the allowlisted branch, bad paths |
| `cargo` | `{command: build\|check\|test\|clippy, package?, release?, test_filter?, nocapture?}` gives a fixed argv: `nice -n 19 <toolchain>/bin/cargo <cmd> --offline --color never -j 2 ...` | run, install, publish and anything else; extra keys; option-looking names and filters |

There is no shell tool. A model asking for `shell` gets `Unknown tool: shell`.

## Order of checks for every call
1. **Spend caps** (`SpendTracker::check` for the session). These are the same caps the agent loop
   already checks before every model call, so a session over its cap cannot keep running cargo.
2. **Per-session call cap** (`HERMES_RS_CODING_MAX_CALLS`, default 200). The parent and its
   `delegate_task` children share it, because the key is the spend session.
3. **Argument allowlist**: unknown keys are refused, values are validated.
4. **Path and branch checks** (`guard.rs`). The branch is read inside the sandbox on every git call.
5. **An audit line** (`allowed`, with the argv) is written *before* the run. If the audit log
   cannot be written, the run is refused.
6. **Sandboxed run** (`exec.rs`). Runs are serialised, one process tree at a time.
7. **An audit line** (`ran`: exit, signal, timeout, wall time, enforced and not-enforced
   sandbox layers, cgroup counters). File contents and command output are never logged, only their sizes.

Refusals get a `refused` line with the reason, and the model gets back `{"error": "refused: ..."}`.

## Sandbox policy (all kinds)
- No network: an empty network namespace plus Landlock TCP deny where the kernel has it
  (`require_net_isolation`, so a run that cannot be isolated does not run).
- Read-only root in a mount namespace. Only the worktree, hermes `state/tmp`, `state/home` and
  (for cargo) `CARGO_TARGET_DIR` and `CARGO_HOME` are writable.
- Landlock read roots: `/usr /bin /sbin /lib /lib64 /etc /proc`, the worktree, the hermes stage
  dir and (for cargo) the toolchain root. Anything else, including `HERMES_HOME`, `.env` and keys,
  gives EACCES.
- `/root`, `/home`, `/run/secrets` and `HERMES_HOME` are hidden behind an empty tmpfs where
  possible. A hidden path that contains the toolchain stays visible and is reported as `partial`;
  Landlock still applies.
- PID namespace, all capabilities dropped, no_new_privs, inherited fds closed, environment
  cleared. The child gets only PATH, HOME, TMPDIR, LANG/LC_ALL/TZ and per-kind variables:
  `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_DIR`, `GIT_WORK_TREE`,
  `GIT_CEILING_DIRECTORIES` for git, and `CARGO_HOME`, `CARGO_TARGET_DIR`, `CARGO_NET_OFFLINE`,
  `CARGO_BUILD_JOBS` for cargo. The OpenRouter key in hermes' environment never reaches a tool.
- Git runs with `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `protocol.allow=never`,
  `credential.helper=` and `commit.gpgsign=false`, plus `--no-ext-diff --no-textconv` for diffs.
- rlimits: fs runs get 10 s wall and CPU; git gets 30 s; cargo gets 600 s wall, 900 s CPU and
  2 GiB RLIMIT_AS. All of these can be set (see below).
- Per-run cgroup when `HERMES_RS_CODING_CGROUP` is set: `memory.max` (cargo 2 GiB, git 256 MiB,
  fs 128 MiB), swap 0, group OOM kill, `pids.max` (cargo 256) and **`cpu.max`** (new on this
  branch in openmind-sandbox: `cgroup_cpu_pct`, cargo 100 % of one CPU by default). If the
  controller is missing, the run fails closed.

## Configuration (environment only; no Hermes config.yaml key)
| variable | default | meaning |
|---|---|---|
| `HERMES_RS_CODING_WORKTREE` | unset = **tools off** | absolute path of the one allowlisted git checkout; must have a real `.git` dir; must not be or overlap `/opt/galion/openmind`, `/opt/galion/hermes`, `/opt/galion/galion-light`, `/opt/galion/bin`, `/root/galion-secrets`, `/etc`, `/usr`, `/var/lib`, ... |
| `HERMES_RS_CODING_BRANCH` | required | the only branch commits may land on (never main/master) |
| `HERMES_RS_CODING_STATE` | `$HERMES_HOME/coding` | stage, tmp, home, cargo-home (must be outside the worktree) |
| `HERMES_RS_CODING_TARGET` | `$STATE/target` | CARGO_TARGET_DIR |
| `HERMES_RS_CODING_AUDIT` | `$STATE/audit.jsonl` | audit log |
| `HERMES_RS_CODING_TOOLCHAIN` | unset = cargo tool refuses | toolchain root with `bin/cargo` (`rustc --print sysroot`), not the rustup proxy |
| `HERMES_RS_CODING_CGROUP` | unset | delegated cgroup dir for per-run cgroups, or `delegate` (see `cgroup_self.rs`) |
| `HERMES_RS_CODING_REQUIRE_CGROUP` | on when CGROUP is set | refuse runs without a per-run cgroup |
| `HERMES_RS_CODING_GATEWAY_MEM_MB` | 64 | memory.max of the gateway leaf in `delegate` mode |
| `HERMES_RS_CODING_JOBS` / `_NICE` | 2 / 19 | cargo `-j` (1-4) and nice (10-19) |
| `HERMES_RS_CODING_CARGO_TIMEOUT` / `_CPU` / `_MEM_MB` / `_CPU_PCT` | 600 / 900 / 2048 / 100 | cargo limits |
| `HERMES_RS_CODING_MAX_CALLS` | 200 | tool calls per session |
| `HERMES_RS_CODING_MAX_READ` / `_MAX_WRITE` | 256 KiB / 512 KiB | file caps |
| `HERMES_RS_CODING_DENY` / `_HIDE` | empty | extra `:`-separated denied worktree roots / hidden dirs |
| `HERMES_RS_CODING_ON_ERROR` | `exit` | `exit`: a bad coding config or preflight refusal exits 2 (old behaviour). `disable`: log it and start **without** the coding tools, chat keeps serving (slot 355). Check any change first with `hermes-gateway --check-config` (validates the coding env and that the coding paths are writable; exit 2 on error). Worktrees may not live under `/var/lib`; use e.g. `/srv/hermes-coding/worktree` + `ReadWritePaths=-/srv/hermes-coding` (docs/ops/hermes-rs-crashloop-r3.md) |
| `HERMES_RS_CODING_ALLOW_NO_LANDLOCK` | unset | `1` lets the tools start on a kernel without Landlock (not recommended) |
| `HERMES_RS_PROVIDER_LOOPBACK_ONLY` | unset | **new in hermes-rs**: `1` makes the curl provider refuse any non-loopback URL. Used by every test here |

Startup (`hermes_tools::preflight`) refuses to turn the tools on when the session or daily spend
cap is 0 (unlimited), when the kernel has no Landlock, or when network isolation is impossible.
hermes-gateway exits with code 2 rather than run with half a policy.

## Building
`hermes-gateway` has a new default feature `coding`. `--no-default-features` builds still work,
with and without `coding`. The crate only adds code. Its dependencies are `hermes-rs`,
`openmind-sandbox` and `serde_json`, all already in the workspace, so there is no new third-party
crate. `hermes-mock-model` is a test binary: a scripted OpenAI-compatible server on 127.0.0.1.

## Limits (honest list)
- Not a hard security boundary: there is no seccomp, and openmind-sandbox says the same. Kernel
  bugs and resource exhaustion inside the caps are out of scope.
- On the dev box the cgroup layer is **not enforced**, because `/sys/fs/cgroup` is root-owned and
  not delegated. Memory and CPU are then bounded by RLIMIT_AS, RLIMIT_CPU, nice and the wall
  timeout only. `cpu.max` and `delegate` mode are code-reviewed, not exercised (see the proposal).
- Spend caps are checked *before* each call, so the last allowed call can overshoot a cap by one
  call's cost. The real test shows $1.20 against a $1.00 session cap. The same is true for the
  existing agent loop.
- `cargo` is `--offline`: crates have to be vendored or already in `$STATE/cargo-home`. Build
  scripts and proc macros run, but inside the same sandbox.
- `fs_write` replaces whole files. There is no patch or delete tool; deletion is a commit of a
  removed file, which needs a write tool that can delete. That was left out on purpose.
