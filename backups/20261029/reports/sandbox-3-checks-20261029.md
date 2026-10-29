# sandbox-3: Security checks the Hermes sandbox should enforce
2026-10-29, about 11:56–12:15 CET (Europe/Zagreb, UTC+1). Agent 3 of 4 (owner t378u, plus rule corrections t379u/t380u).
**Scope:** box-only tests and read-only VPS checks over `ssh slavik-vps`. Nothing live was changed. No git was run in /opt/galion/openmind. No push, no deploy, and no paid OpenRouter calls.
**Code reference:** worktree `wt/hermes-model-picker` @ dff00cbe (newest swarm tip on the box).
**Box artefacts:** `/workspace/swarm/scratch/sandbox-3-checks/` (lltest crate, diff-policy.sh, bad.diff, real.diff).

## Pipeline these checks assume (t379u/t380u)
1. Hermes produces a change on a branch.
2. **Sandbox deploy**: a real, live-style deployment to an isolated copy of the platform.
3. **Security checks** (below) run against both the diff and that sandbox deployment.
4. If every check passes, a **gated live apply** follows. Hermes itself never holds the live credentials.

## VPS kernel and host facts (measured today)
| Item | Result |
|---|---|
| Kernel / virt | 6.8.12-43-pve, **LXC** container (`systemd-detect-virt` = lxc) |
| Active LSMs | `lockdown,capability,yama,apparmor`. **Landlock is not in the list.** |
| Landlock syscall | `landlock_create_ruleset(NULL,0,VERSION)` → **-1 EOPNOTSUPP**. Landlock is unavailable, and fixing this needs a Proxmox host change (`lsm=` on the host kernel cmdline). It cannot be done from inside the LXC. |
| Seccomp | The process already runs under the LXC filter (`Seccomp: 2`, 1 filter). A **throwaway perl process stacked a second filter** (NNP=1, `Seccomp_filters: 2`), and `getpid` then returned **-1 EPERM**. **Stacked seccomp works.** |
| User namespaces | `max_user_namespaces`=2147483647, `unprivileged_userns_clone`=1, `apparmor_restrict_unprivileged_userns`=0. These are available. |
| nftables | v1.0.9. Tables: inet filter, ip/ip6 filter, inet f2b-table, **inet galion_stt_egress** |
| Per-user egress model | `meta skuid 994 oif != "lo" drop` (uid 994 = galion-stt). **Tested:** an outbound TCP connect as uid 994 (setpriv, throwaway) timed out, and the drop counter went from 9 to 13 packets. As root, the same connect was OPEN. **Per-uid egress drop works.** |
| Gap in that model | As uid 994, a connect to **127.0.0.1:8642 was OPEN**. The rule exempts `lo`, so a per-uid nft rule does **not** protect the live loopback ports. |
| Live ports | 8642, 18080, 8080, 8651 and 11434 all listen on **127.0.0.1 only**. Any sandbox that shares the host network namespace can reach them, so it needs an empty netns (or an nft `lo` dport rule for the sandbox uid). |
| hermes-rs.service | `User=` empty (**runs as root**), NoNewPrivileges=yes, ProtectSystem=strict, ProtectHome=read-only, PrivateTmp=yes, ReadWritePaths=/var/lib/hermes-rs, MemoryMax=64M, TasksMax=64, **no SystemCallFilter, no RestrictAddressFamilies, no IPAddressDeny, PrivateNetwork=no**, full capability bounding set. `systemd-analyze security` = **8.8 EXPOSED**. |
| VPS spend drop-in | 70-research-budget.conf: `HERMES_RS_SESSION_USD_CAP=5.00`, `DAILY_USD_CAP=7.00`, `DAILY_CALL_CAP=2000` |
| VPS tooling | cargo, cargo-deny and gitleaks were **not found** on the VPS PATH, so the Rust checks must run on the box or in CI. |
| Sandbox presets | `/opt/galion/openmind/deploy/sandbox/presets/{build-tool,net-tool,read-only-tool}.toml` exist (`ls` only), but as of t294u no live process loads them. |

## Checks table
| # | Check | Exists? | Where | Tested result (today) |
|---|---|---|---|---|
| **Code review / diff policy** |||||
| 1 | Forbidden-path diff policy (systemd units/drop-ins, /etc, keys, `.env`/link.env, push-branch.sh / remote-push.sh, /opt/galion/openmind, cargen, .github/workflows, .cargo/config, build.rs, deny.toml) | **Partial.** Runtime only: hermes-tools `guard.rs` NO_GO `.git` and NO_WRITE `.cargo .github .gitlab-ci.yml .gitmodules`, plus `config.rs` DENY_ROOTS (/opt/galion/openmind, /opt/galion/hermes, /root/galion-secrets, /etc, /usr, …) for the worktree location. **There is no diff-level policy.** | wt/*/crates/hermes-tools/src/{guard,config}.rs | **Prototype built:** `scratch/sandbox-3-checks/diff-policy.sh`. Sample bad.diff → **FAIL** (exit 1). It flagged cargen, link.env, systemd unit and push-branch.sh. A real commit diff (dff00cbe, 1930 lines) → **PASS** in 4.3 s, with no false positives. |
| 2 | Secret scan | **Yes, in CI and the repo.** repo-check `secrets` (token_shaped, public_ipv4s); gitleaks in `.github/workflows/self-eval.yml` and `launch-gates.yml`. gitleaks is **not installed** on the box or the VPS. | crates/repo-check, .github/workflows | The prototype scan of added lines flagged the runtime-built fake OpenRouter key and GitHub PAT (lines 5 and 6) and a public-IP-shaped line (line 7, TEST-NET). The real diff was clean. The repo-check/gitleaks binaries themselves were not rerun today. |
| 3 | Licence / ban check | **Yes.** `deny.toml` licence allowlist (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, 0BSD, CC0) and bans (openssl, plus the cudarc/metal/… bans from t276). cargo-deny is **not installed** on the box. | deny.toml, CI launch-gates | `cargo tree --offline -p hermes-tools` checked against the allowlist: 35 crates, **0 violations**. Caveat: it is OR-only, so AND expressions are approximated. cargo-deny was not run. |
| 4 | cargo fmt / clippy / test | **Yes, in CI** (self-eval.yml). Older box logs: hc-clippy.log, hc-test.log. | .github/workflows/self-eval.yml | Not rerun today. t287u-B (10-27): 70 pass, 0 fail, 10 ignored, 3 soft-skips. The CI gap is unchanged: adversarial tests are `#[ignore]` or skip under CI. |
| **Permission limits** |||||
| 5 | Landlock filesystem allowlist | **In code** (openmind-sandbox, ABI≥4 also handles net). **Not live.** | crates/openmind-sandbox/src/lib.rs | **Box:** the new `lltest::landlock_denies_write_outside_allowlist` test passes. A write in the allowlisted dir works, and a write outside it gets EACCES with no file created. **VPS: EOPNOTSUPP, so it cannot be enforced there.** |
| 6 | seccomp | **Not in code.** openmind-sandbox says "No seccomp" (lib.rs:30,39). | — | **Box:** `lltest::seccomp_filter_denies_syscall_with_eperm` passes. **VPS:** a stacked filter works (getpid → EPERM). |
| 7 | No network except an allowlist | **In code:** empty net namespace when `net=false`, and Landlock net on ABI≥4. Per-uid nft exists only for galion-stt. | openmind-sandbox; VPS `inet galion_stt_egress` | Box: t287u-B real_escape tcp/udp CONTAINED. VPS: the per-uid drop works, but loopback stays open (see facts above). |
| 8 | Dedicated user | **No.** hermes-rs runs as root (`User=` empty). | VPS unit | Read-only check (systemctl show). |
| 9 | CPU / mem / time caps | **Yes in code.** hermes-tools cargo_timeout 600 s, cargo_cpu 900 s, cargo_mem 2048 MB, cpu_pct 100, git 30 s, fs 10 s, workdir quota 512 MB (16–4096 MB), and the per-run cgroup. Unit: MemoryMax=64M, TasksMax=64. | hermes-tools/src/config.rs:92,286-291 | Box t287u-B: forkbomb/membomb CONTAINED under the delegated cgroup. The VPS quota tests (q1–q9) have never run. |
| 10 | Read-only root | **Yes in code** (mount namespace ro_root). The unit has ProtectSystem=strict. | openmind-sandbox | Box t287u-B: ro_root Enforced, mount/remount-root CONTAINED. |
| 11 | No access to live sockets/ports (8642/18080/8080/8651/11434) | **Only via the empty netns.** No nft or unit IPAddressDeny. | — | All 5 ports are on 127.0.0.1. Reachable from uid 994 despite the egress rule (8642 tested OPEN). |
| **No live access** |||||
| 12 | No VPS ssh keys or tokens inside the sandbox | **Partial.** hermes-tools `hide` includes $HOME, and DENY_ROOTS includes /root/galion-secrets. | hermes-tools/src/config.rs:299 | The box has `~/.ssh/id_ed25519_vps`, and push-branch.sh uses it. The sandbox runner must not run as a user that can read it. No test covers this. |
| 13 | Push helpers unreachable | **No guard** beyond the branch-name check `swarm/*` in push-branch.sh. | /workspace/swarm/push-branch.sh, remote-push.sh | Covered by the forbidden-path rule in the prototype only. |
| **Spend** |||||
| 14 | Hermes $20 total cap | **Yes, research helper only:** `bin/hermes-research` LIMIT=20 refuses at a logged total ≥ $20. The logged total is now $0.0036. | /workspace/swarm/bin/hermes-research | Read only; not exercised. |
| 15 | Warn at $16 | **No.** There is no warn threshold anywhere (helper or hermes-rs spend.rs). spend.rs only blocks and emits a nerve `spend_cap` event on a hit. | hermes-rs/src/spend.rs | — |
| 16 | Runtime caps | **Yes.** spend.rs session/daily USD + daily calls. Code defaults are 1.0 / 2.0 / 500; VPS overrides are 5.00 / 7.00 / 2000. | hermes-rs/src/{spend,config}.rs | **Conflict:** the VPS daily cap of $7 is above the new **$5 OpenRouter limit for everything** (t380u). The $20/$16 figures are also above $5. |
| **Model-tier routing (t379u/t380u)** |||||
| 17 | Deep vs routine model routing | **Knobs exist; there is no tier policy.** The default model is `anthropic/claude-opus-5.5` (high cost). The config also has `model_routes`, `delegation.model`, `background_model`, `free_only_background=true`, `cron_model` and `reasoning_overrides`. | hermes-rs/src/config.rs:140-170 | Read only. Routine/background work is free-only by default, but interactive and sandbox runs default to the top model. |

## Recommended mandatory set (the gate before any live apply)
**A. Static, on the diff (box/CI, before the sandbox deploy):**
1. Forbidden-path policy (diff-policy.sh rules, ported into repo-check as a `diff` subcommand). Hard fail.
2. Secret scan: repo-check `secrets` plus gitleaks on the diff and the tree. Hard fail.
3. cargo-deny licences + bans. Hard fail.
4. `cargo fmt --check`, `clippy -D warnings`, `cargo test`. Adversarial suites must actually run on a real runner (no `#[ignore]` or CI skip), and skips count as a fail.

**B. Sandbox deploy (isolated platform copy):**
5. Runs as a dedicated non-root uid, with no readable ssh keys, tokens or link.env (prove it with a test that tries to read them).
6. Empty network namespace by default, plus a per-uid nft egress allowlist modelled on `galion_stt_egress`. **Add** an `oif lo tcp dport {8642,18080,8080,8651,11434} drop` rule for that uid, because the existing model leaves loopback open.
7. Seccomp filter (stacking verified on the VPS LXC), because **Landlock is unavailable on the VPS**. Until the Proxmox host enables Landlock, filesystem confinement on the VPS has to come from the mount namespace with a read-only root, plus DAC/uid.
8. cgroup CPU/mem/pids caps, plus wall-clock and workdir quotas (the existing hermes-tools defaults), and the VPS quota tests q1–q9 run for real.
9. Health and smoke checks against the sandbox copy's own ports, never the live ones.

**C. Spend (checked before every provider call):**
10. Total cap at or below the **$5 OpenRouter limit** (t380u supersedes the $20/$16 figures for now), with a warn at 80% ($4). Lower the VPS daily cap from $7 to ≤$5 (a live change, which needs owner approval).
11. Tier routing: routine/sandbox/cron/background runs go to cheap or free models (`free_only_background`, `background_model`, `cron_model`, `delegation.model`). Only explicit deep-reasoning tasks go to the high tier (`model_routes`/`reasoning_overrides`). The gate should fail any routine run that used the high-tier default.

**D. Gated live apply:** applied only by a separate approver identity holding the live credentials, after A–C pass. Hermes never has them.

## Built / tested today (box only)
- `scratch/sandbox-3-checks/lltest`: 2 Rust tests (Landlock denies a write, seccomp gives EPERM). `cargo test --offline`: **2 passed, 0 failed.**
- `scratch/sandbox-3-checks/diff-policy.sh`: bad.diff → FAIL (4 forbidden paths, 2 secrets, 1 IP). Real diff → PASS.
- cargo-tree licence check on hermes-tools: 0 violations.
- VPS (throwaway processes only): Landlock EOPNOTSUPP, seccomp stacking OK, uid-994 egress drop OK, loopback 8642 open.

## Remaining / not done
- Landlock on the VPS needs a Proxmox host kernel `lsm=` change (live, owner-only).
- seccomp is not implemented in openmind-sandbox.
- hermes-rs runs as root with exposure 8.8. A dedicated user and a syscall filter are needed (live unit change, not done).
- There is no $16 (or $4) warn threshold, and the VPS daily cap ($7) is above the $5 OpenRouter limit.
- gitleaks and cargo-deny are not installed on the box or VPS, so they were not run today.
- The diff-policy prototype is shell. It should become a repo-check subcommand with unit tests.
