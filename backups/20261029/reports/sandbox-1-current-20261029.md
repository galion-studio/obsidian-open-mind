# Sandbox agent 1 of 4: current sandbox implementation and tests (2026-10-29)
Owner t378u (corrections t379u/t380u). Checked 11:55–12:15 CET (Europe/Zagreb, UTC+1).
This was read-only work. Nothing on the VPS changed. I ran no git in /opt/galion/openmind, did not touch /workspace/cargen, made no paid or OpenRouter calls and never used the approval-retry flag. This report contains no secrets and no VPS IP.

## 1. Summary
- **No sandbox is wired into live Hermes at runtime.** The live hermes-rs runs `hermes-gateway-b7fbbc78`, and it has `hermes-tools` and `openmind-sandbox` compiled in, but the coding runtime is OFF: `HERMES_RS_CODING_WORKTREE` is not set in the unit or in the process env.
- **The VPS kernel has no Landlock.** `6.8.12-43-pve`, LSM=`lockdown,capability,yama,apparmor`, LXC guest. The box does have Landlock (`6.12.94+`, LSM=`capability,landlock,selinux,bpf`).
- **The Landlock workaround (D-320-1 a/b/c) and the test gates exist only on GitHub branches.** None of them are on `main`, on the box worktrees or on the VPS.
- **The current sandbox cannot do "live-style deployments" (owner rule t379u).** openmind-sandbox runs one command or tool (fs/git/cargo) in a confined workdir. Nothing deploys an isolated copy of the platform (units, ports, data) inside it. The live-style deploy testing that exists today happens elsewhere: GALION_REAL `real_*` tests on alternate ports on the box, plus `galion-deploy` (backup/install/health/rollback) applied by an owner-approved human or agent. Hermes output does not pass through any of it.
- **Box tests: 29 pure tests passed and 0 failed.** I did not run the 56 tests that actually execute the sandbox. Standing rule `sandbox_vps_only` (RULES.md §47) forbids that on the box (see §4).

## 2. Components (path | what it does | state)
| # | Path | What it does | State (verified 2026-10-29) |
|---|---|---|---|
| 1 | `crates/openmind-sandbox` (box worktrees, lib.rs md5 8d793551 in all 14 newest wt; e.g. wt/hermes-model-picker dff00cbe) | Runs one command with rlimits (CPU/AS/NOFILE/FSIZE), a cleared env, a fixed workdir, a wall timeout, Landlock fs/TCP, user/mount/pid/net namespaces, a /proc mask (LXC fallback), a hide list (/opt/galion), a private tmpfs workdir quota with copy-back, and per-run cgroup v2. No seccomp. | Code only. On GitHub `main` 7ba85b60 (crate listed). |
| 1b | VPS `/opt/galion/openmind/crates/openmind-sandbox` | Source copy (has tests/preset.rs and tests/sandbox.rs) | lib.rs md5 is now **d30d88e4** (it was 204b582a on Oct 27). That equals the box console/hades/release variant, **not** the newest 8d793551. Dir mtime is Oct 27 18:42 (VPS clock). Nothing loads it except via the gateway build. |
| 2 | `crates/hermes-tools` (exec.rs, guard.rs, config.rs, tools.rs, cgroup_self.rs) | Hermes coding tools (fs/git/cargo) run through openmind-sandbox. Worktree only, allowlisted argv, deny paths (refuses a worktree under /var/lib), never push, audit log, spend/call caps, 512 MiB workdir cap. Refuses to start without Landlock unless `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`. | Compiled into live `hermes-gateway-b7fbbc78`: `strings` shows 4 `landlock` hits, `coding: refusing to start`, and all `HERMES_RS_CODING_*` names incl. ALLOW_NO_LANDLOCK. **Runtime OFF.** VPS has no `crates/hermes-tools` source. |
| 3 | `deploy/sandbox/presets/{build-tool,net-tool,read-only-tool}.toml` | Per-tool policy | Present on the VPS (mtime Oct 27 18:43). No running process loads them. |
| 4 | GitHub `swarm/coding-tools-landlock-stage` f68def1d (pushed 2026-10-28 16:05) | `HERMES_RS_CODING_NO_LANDLOCK=refuse\|allow\|layered`. Option c `exec::layered_check` fails closed unless userns, mountns, pidns, net, ro_root, hide_home and workdir_quota are all enforced. Staged drop-ins `landlock-{a-allow,b-refuse,c-layered}.conf` are **not installed**. | compare main...branch: diverged, ahead 3 / behind 121 (base 98466ab9). **Not on main, not on box, not live.** Live binary has 0 `HERMES_RS_CODING_NO_LANDLOCK` / `layered_check` strings. Waiting on owner decision D-320-1. |
| 5 | GitHub `swarm/hermes-sandboxed-require` 4828997d | `cargo xtask test --sandbox=auto\|require\|force`. require makes sandboxed.rs FAIL instead of SKIP. | ahead 2 / behind 120. Not on main. |
| 6 | GitHub `swarm/realtest-real-env-guard` a98a75bc | New crate `vps-sandbox-gate`: real sandbox tests need GALION_REAL=1 + GALION_REAL_SANDBOX=1 + a VPS host check, else SKIP. Includes lint `every_sandbox_real_test_is_gated`. | ahead 16 / behind 120. Not on main. |
| 7 | Slot 444 `hermes-tools-sandbox-gate` (plans/wave4-tasks.md) | Would gate the 8 plain sandboxed.rs tests (D-400-2) | Row exists. **Branch does not exist on GitHub (404).** Not done. |
| 8 | GitHub `staged/sandbox-v1` 2eeed08 (local /workspace/oom-ws/staged-sandbox-v1) | openmind-sandbox v1 from the VPS tree, Oct 18. Commit says 24 tests, 3 of 4 box runs green, 1 intermittent /proc failure. | Staged and waiting on Slavik (OMNODE-2). No `STAGED.md` at the branch root; it is `TEACHER/STAGED.md`, which has no sandbox entry. Superseded by #1. |
| 9 | `edge/openmind-node/src/seccomp_table.rs` | The only seccomp code: a cross-arch deny-list | Edge prototype. Not used by the Hermes path. |
| 10 | `wt/galion-os-sandbox-proto/crates/galion-box` 4374605 | Rootless launcher (namespaces/Landlock/rlimits) | Prototype. Box only. |
| 11 | `crates/galion-deploy` | backup → atomic install → restart → health → auto-rollback. Rollback journal. | This is the actual live-apply tool used by the Oct 29 deploys (reports/deploy-430-…). It has no sandbox stage and no Hermes input. |
| 12 | `crates/galion-realtest` + per-crate `real_*` tests | Start real binaries on alternate ports with GALION_REAL=1 (closest thing to a "sandbox deploy") | Run on the box per deploy. Not wired to Hermes. |
| 13 | VPS `/srv/hermes-coding-worktree` (root, .git + README.md, created Oct 27 19:46 CET) | Leftover from the overnight coding-enable attempt | Exists. `/srv/hermes-coding` (slot 397 target) is absent. |
| 14 | VPS hermes-rs drop-ins: 50-rust-bin, 60-nerve-url, 70-research-budget, 90-deploy-t291u | Line 3 of 90-deploy-t291u says "Coding RUNTIME OFF … Landlock absent … do not enable coding without ALLOW_NO_LANDLOCK" | ProtectSystem=strict, ReadWritePaths=/var/lib/hermes-rs, NoNewPrivileges=yes, SystemCallFilter empty. No sandbox/sbx/coding units. bwrap/firejail/nsjail absent on the VPS (box has /usr/bin/bwrap, unused). |
| 15 | VPS `/opt/galion/arena/homes/g2-*/sandbox.json` (from t287u-A) | Policy declarations only, no enforcer | Advisory |

Commands used (VPS, read-only, `ssh slavik-vps`): `uname -r`, `cat /sys/kernel/security/lsm`, `systemd-detect-virt` (→ lxc), `systemctl show hermes-rs -p ExecStart,DropInPaths,ProtectSystem,ReadWritePaths,NoNewPrivileges,SystemCallFilter`, `grep HERMES_RS_CODING` on the drop-ins, env var names from `/proc/<pid>/environ` (values not read: none matched CODING/SANDBOX/LANDLOCK), `strings` on the gateway, `curl 127.0.0.1:8642/health` (→ ok, hermes-rs 0.1.0), `ls`/`md5sum`, `journalctl -u hermes-rs | grep coding`.
The journal confirms the Oct 27 enable attempt failed. From 19:45:51 CET it logged `worktree /var/lib/hermes-rs/coding-worktree overlaps the denied path /var/lib` 7 times, then at 19:46:15 CET `mkdir /srv/hermes-coding-state/stage: Read-only file system`. After that the attempt was rolled back.

## 3. Kernel Landlock
| Host | uname -r | /sys/kernel/security/lsm | Landlock |
|---|---|---|---|
| box | 6.12.94+ | capability,landlock,selinux,bpf | yes (87 kallsyms hits; max_user_namespaces 63991) |
| VPS | 6.8.12-43-pve (LXC guest) | lockdown,capability,yama,apparmor | **no**. The host (PVE) kernel decides this; the guest cannot enable it. max_user_namespaces 2147483647, cgroup2fs. |

## 4. Tests run on the box
Source: `git archive` of wt/hermes-model-picker @ dff00cbe (newest box tree, sandbox md5 8d793551) into `/workspace/swarm/scratch/sandbox-1-20261029`, with its own `CARGO_TARGET_DIR`. GALION_REAL and GALION_REAL_SANDBOX were unset.
`timeout 900 nice cargo test -p openmind-sandbox -p cgroup-detect -p hermes-tools --no-run -j4` compiled everything: OK, 5.3 s, 14 test binaries.

**Rule conflict, resolved conservatively:** the task said "run the sandbox's tests on the box", but RULES.md §47 (`sandbox_vps_only`, Slavik via Mate, 2026-10-21) says sandbox tests are never run on the box and the VPS is read-only for this task. So I executed only binaries whose source states they start no sandbox:

| Binary | Result |
|---|---|
| openmind-sandbox lib unit (mask, status bits, treecopy) | 9 pass / 0 fail |
| cgroup-detect lib unit (parsers, delegation logic) | 8 / 0 |
| cgroup-detect tests/fixture.rs | 1 / 0 |
| hermes-tools tests/guard.rs ("No process is started") | 6 / 0 |
| hermes-tools tests/workdir_quota.rs ("Nothing here starts the sandbox") | 5 / 0 |
| **Total executed** | **29 pass / 0 fail / 0 ignored**, all rc 0 (log: scratch/sandbox-1-20261029/run-pure.log) |

Compiled but **not executed** (they run the sandbox or probe the live host):

| Binary | Tests | Note |
|---|---|---|
| openmind-sandbox tests/sandbox.rs | 31 | runs the sandbox |
| openmind-sandbox tests/preset.rs | 3 | runs the sandbox |
| openmind-sandbox tests/real_escape.rs | 1 | `#[ignore]`, GALION_REAL |
| openmind-sandbox tests/real_workdir_quota.rs | 9 | all `#[ignore]`, VPS only |
| hermes-tools tests/sandboxed.rs | 10 | skips when CI is set |
| hermes-tools tests/real_ticket.rs | 1 | GALION_REAL |
| cgroup-detect tests/detect.rs | 1 | live /proc and /sys probe |
| **Total not executed** | **56** | |

The hermes-tools lib has 0 unit tests.

Latest sandbox-executing results on record (not re-run today):
- **Box, Oct 27 (t287u-B):** 70 pass / 0 fail / 10 ignored / 3 soft-skip.
- **VPS, Oct 27 22:55 CET (slot 320), uid 65534:** hermes-tools sandboxed plain 2/8. The 8 were refused by preflight (no Landlock, fail closed). With ALLOW_NO_LANDLOCK=1: 10/0.
- **VPS, Oct 28 01:19–01:33 CET (slot 334):** staged_preset 15/0 ×3. Option c layered: 14 layers enforced, `/health` 200 "tools on". Option b: exit 2 "refusing to start".
- **VPS, Oct 28 03:28 CET (slot 346):** CI=1 require FAILED 2/8 "host lacks landlock" (as designed).

## 5. Can the current sandbox do real ("live-style") deployments? (t379u)
**No.** These facts support that:
- openmind-sandbox's unit of work is one process (tool call) with a ≤512 MiB tmpfs workdir. There is no notion of a service, a unit, a port set, a data snapshot or a health check.
- hermes-tools allows only fs/git/cargo in a worktree, with "never push". It cannot install or start binaries, and the deny list blocks /var/lib.
- Deploy-like testing exists only outside the sandbox: `galion-realtest`/`real_*` tests (real binaries on alternate ports, e.g. 23000–23055 on Oct 29), the `galion-deploy` install/rollback tool, and `cargo xtask release-recipe` (0df394ce, a reproducible build). These are run by swarm agents under owner approval, not by Hermes.
- The VPS cannot run the full sandbox anyway: no Landlock, LXC /proc gap (handled by the Mask fallback), and ProtectSystem=strict blocked the coding state dir on Oct 27.

## 6. Model-tier routing (t379u/t380u)
- The sandbox and hermes-tools contain **no** model selection; tools are model-agnostic. Routing lives in the hermes-rs config.
- Live (reports/model-routing-20261029.md, t360u):
  - default/research: OpenRouter claude-opus-5.5
  - delegation: claude-sonnet-5.5
  - auxiliary: free models
  - `galion` route: claude-haiku-5.5
  - cortex: River DeepSeek Flash
- The sandbox gate has no place to mark a task as "deep" (high-tier) versus "routine" (low-cost) yet. Under the new $5 OpenRouter limit, a sandbox deploy/test stage must not call any model by itself. All checks found here (tests, gates, galion-deploy) are LLM-free.

## 7. Gaps
1. No Hermes → sandbox → security check → gated live apply pipeline exists. The coding runtime is off, and nothing routes Hermes output to a sandbox deploy.
2. There is no "sandbox deploy" stage (an isolated platform copy). The closest pieces are galion-realtest real_* and galion-deploy.
3. The VPS has no Landlock. D-320-1 (a allow / b refuse / c layered, c recommended) is still an owner decision. f68def1d is not on main or the VPS.
4. Three sandbox branches are pushed but not on main: f68def1d, 4828997d, a98a75bc (diverged; behind main by 120–121).
5. Slot 444 (gate the 8 plain sandboxed.rs tests, D-400-2) was never run; there is no branch.
6. Version drift: VPS sandbox source d30d88e4 ≠ newest box 8d793551. The 14 newest box trees agree on 8d793551.
7. There is no seccomp on the Hermes path (only the edge deny-list table).
8. CI gaps from t287u-E are still open: real tests are ignored or skipped in CI, and there is no VPS/self-hosted runner.
9. VPS leftover `/srv/hermes-coding-worktree` (root-owned git dir) from Oct 27. `/srv/hermes-coding` (slot 397 staged path) does not exist.
10. Process rule conflict: "run sandbox tests on the box" vs `sandbox_vps_only`. An owner ruling is needed before anyone runs the 56 sandbox-executing tests, on the box or on the VPS as uid 65534 in /var/tmp/sandbox-test-*.
