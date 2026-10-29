# hermes-gate: rebuild, VPS deploy, autonomous demo, backup (2026-10-29)

All times are Europe/Zagreb (CET, UTC+1). The VPS clock shows UTC; its times are converted here.
Written by Grok Bot for Mate. This report has facts only: commands run and what they returned.
It contains no secrets and no VPS IP.

Approval note: the brief says the job was approved in chat at 13:40 CET. That chat message was not
among the owner messages passed to this job. The only one passed was pt386u, about the rollback investigation.

## Step 1: CPU drop-in (skipped, as steered)
- Not created, restarted or killed by this job. I ran no systemctl, kill or restart against hermes-rs.
- Read-only check at 14:20 CET: `/etc/systemd/system/hermes-rs.service.d/95-cpu-always.conf` exists, sha256 starts `5014b75c95c9`.
  Content and rollback are in the backup at `backups/20261029/dropin/`.
- Seen in the journal, not caused by this job: hermes-rs was stopped and started by systemd at 14:07:42, 14:10:22 and 14:17:25 CET.
  MainPID is now 2239926, not the 2127348 given in the brief. Another job is restarting it.

## Step 2: gate rebuilt and extended
Starting point: e2b68411 (the original gate, 12:11:25 CET), cherry-picked onto main 7ba85b60 and then extended.
Both versions are kept:
- `swarm/hermes-sandbox-gate` = e2b68411, unchanged (pushed as is).
- `swarm/hermes-sandbox-gate-v2` = 06d20dca, based on main 7ba85b60. Commits:
  - 2f8cf06: cherry-pick of e2b68411;
  - bb442d2: v2;
  - e5ea615: ledger dir fix;
  - 6194e4af: plan env check fix + VPS plan;
  - 5b986fa: VPS cargo wrapper;
  - 06d20dc: tests split.

What v2 adds or changes:
- **openmind-sandbox: new `seccomp` layer** (BPF deny-list).
  - Denied with EPERM: ptrace, process_vm_readv/writev, mount, umount2, fsopen, fsconfig, fsmount, fspick, move_mount, open_tree, mount_setattr, pivot_root, chroot, setns, unshare, init/finit/delete_module, kexec_load/kexec_file_load, bpf, perf_event_open, keyctl, add_key, request_key, userfaultfd, io_uring_*, swapon/swapoff, reboot, settimeofday, clock_settime, clock_adjtime, adjtimex, sethostname, setdomainname, name_to_handle_at, open_by_handle_at, acct, quotactl, syslog, ioperm.
  - Denied with other errors: clone3 → ENOSYS; clone with namespace flags → EPERM; socket families other than unix/inet/inet6/netlink → EAFNOSUPPORT; a wrong arch or the x32 ABI kills the process.
  - It fails closed if it was requested and could not be loaded.
  - Other fixes: status bits widened to 64; nested hide paths deduplicated (before this, hide_home failed when a hidden dir held another hidden dir); dirs the user cannot enter are not mounted over.
- **Landlock is never used.**
  - The gate sets `landlock=false`. A plan that asks for Landlock is refused, and so is a plan that drops seccomp, net or no_new_privs.
  - SECURITY REVIEW: `loopback_tcp_ports` (from e2b68411) is not enforced by the kernel without Landlock. The boundary is the private network namespace, which has only `lo`.
- **7 static checks:** applies_to_base, allowed_tree, protected_targets, no_secrets, no_new_capabilities, size_limits, text_only. All 7 must pass before any build runs.
- **Stages are isolated** with seccomp, user/pid/mount/net namespaces, a read-only root, rlimits, a cleared env, no capabilities and no_new_privs.
  - Hidden from every stage: /opt/galion, /srv, /home, /root, /var/backups, every /var/lib child except galion-gate, the sandbox key dir, /workspace/swarm/secrets and the home secret dirs.
  - The source repo's git dir and the worktree's .git link are also hidden or removed.
- **Pass record:** Ed25519-signed by the sandbox key. It contains proposal sha, patch sha, plan sha, the 7 checks, per-stage layer enforcement, the result tree and the verdict.
- **Owner approval:** signed with a separate key over proposal sha, record sha, target commit, expiry and a label.
- **apply:**
  1. Refuses live paths for source, staging root and ledger before any command runs (the test asserts zero git spawns).
  2. Checks the replay ledger, both signatures, sandbox key ≠ owner key, pass verdict, checksums, target commit = tested base = approved commit, and expiry.
  3. Applies only into a fresh `git worktree --detach` under the staging root (index only, no commit).
  4. Reverses the change and fails if the tree differs from the tested tree.
- **CLI:** `keygen | propose (--worktree or --base-commit + --patch) | gate | verify | approve (--label) | apply`.

Tests (box, rust 1.99.1):
- `cargo test -p hermes-gate`: **17 passed**, 0 failed. One test per refusal:
  - refusal_no_approval
  - refusal_sandbox_key_used_as_owner_key
  - refusal_replay
  - apply_refuses_live_paths_before_running_any_command
  - refusal_expired_approval
  - refusal_checksum_mismatch
  - refusal_target_moved
  - refusal_result_differs_from_tested_tree_reverts
  - plus the full flow, the escape probe (seccomp on, unshare denied) and the static-check cases.
- `cargo test -p openmind-sandbox`: **48 passed**, 0 failed, 10 ignored (GALION_REAL-only). Breakdown: lib 10, loopback 2, preset 3, sandbox 31, seccomp 2.
- `cargo clippy -p hermes-gate -p openmind-sandbox --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `repo-check`: 2313 files, 0 errors, 86 warnings (all `utc` warnings in other crates) → PASS.

Real run on the box. The proposal was a one-line change to `crates/hermes-rs/src/tools.rs`, ClockTool description: "Current time in Europe/Zagreb (RFC 3339 with offset)." → "Current date and time in Europe/Zagreb (RFC 3339 with UTC offset)."
- Record time 14:04:46 CET; build+test+deploy stage time 24.7 s.
- Checks: 7/7 PASS.
- Build of `hermes-gateway`: PASS, 17.2 s.
- Test `hermes-rs --lib`: PASS, 48 passed, 7.5 s.
- Deploy smoke (gateway `--mock` on loopback 18642 inside the sandbox): `/health` status ok, chat HTTP 200, no-key `/v1/models` HTTP 401, no-key chat HTTP 401.
- Layers on every stage: seccomp on, landlock off (by policy), net/pidns/mountns/ro_root/hide_home/caps on.
- Verdict pass. Record sha256 fa451547…, tree 7608cb09…
- verify: pass. Approve (owner key, label "box real run approval"), then apply: staged copy, tree matches.
- Second apply: "replay refused". `--source /opt/galion/openmind`: "live path refused". Source repo unchanged (0 modified files).

## Gate on the VPS (installed)
- System user `galion-gate` (uid 993, nologin, no sudo).
- Binary `/opt/galion/gate/bin/hermes-gate`: root:root 755, sha256 starts `9ab5b706ac60`. Built on the VPS from 6194e4af; later commits only change tests and plans.
- `/var/lib/galion-gate` (galion-gate, 750):
  - `keys/sandbox.seed`: galion-gate, 600; the keys dir is 700.
  - `sandbox.pub`, `owner.pub`, `demo-owner.pub`: root:root 644.
  - `repo/`: the gate's own clone of v2.
  - `toolchain/`: own rustup 1.99.1 + crate cache, 363 MB.
  - `plans/`, `runs/`, `scratch/`, `staging/` (plus `applied.ledger`, 600).
- The owner key is not on the server. It is on the box at `/workspace/swarm/secrets/hermes-gate/owner.seed` (600, dir 700) and was never printed.
- The box kernel has Landlock (ABI 6) but the gate does not use it. The VPS kernel LSM list is `lockdown,capability,yama,apparmor`: no Landlock. Records report `kernel_landlock_abi: 0`, `landlock: not used`.
- Unprivileged user namespaces work for galion-gate.
- Problem found and fixed: in this LXC the sandbox cannot mount a fresh /proc (lxcfs). The masked /proc has no `/proc/self`, so rustup proxies, rustc's rpath and rust-lld failed ("no /proc/self/exe").
  - Fix: `plans/vps-cargo-noproc.sh` calls the toolchain binaries directly, with LD_LIBRARY_PATH, `--sysroot` and `-Clinker-features=-lld`.
  - Also fixed: the plan env check now looks at each PATH part, so the gate home is allowed and live roots are still refused.
- VPS real run (same clock change; base 6194e4af), record time 14:12:31 CET:
  - Checks 7/7 PASS.
  - Build PASS 24.9 s.
  - Test PASS, 48 passed, 11.4 s.
  - Deploy smoke: health ok / chat 200 / no-key models 401 / no-key chat 401.
  - Seccomp on and landlock off on every stage. Verdict pass, record 2c080b3f…
  - Approved on the box with the owner key, then applied on the VPS. Staged at `/var/lib/galion-gate/staging/clock-desc-vps-256ab4ce`; tree 33863da2 matches.
  - Replay refused. `--source /opt/galion/openmind`, `--staging-root /opt/galion/rust/src/x` and `--source /var/lib/hermes-rs` were all "live path refused", and nothing was created there.

Not done yet. Needed to make the gate the mandatory path:
1. hermes-rs still runs as root with full capabilities, so it could skip the gate. Run it as a non-root user that cannot write live code, the systemd units or `/var/lib/galion-gate`.
2. Make galion-deploy (and any switch script) accept only a commit/tree that has a valid pass record and owner approval for that exact tree. Pin `owner.pub` in the deployer, not as a CLI argument.
3. Add branch protection on main plus a CI job that runs the gate.
4. Add a cgroup for gate stages (now "off": no pids/memory limit for the whole tree, only rlimits).
5. Have a security review of: loopback ports not kernel-enforced; `/home/box` visible on box runs because the toolchain lives there (its secret subdirs are hidden); the read-only view of `/var/lib/galion-gate` (the keys dir is hidden; staging and ledger are visible read-only).
6. Seccomp denials return errors and are not logged, so they cannot be counted from kernel logs. A logging mode would be needed for audit.

## Step 3: autonomous demo (no human check-ins)
- Spend before: sessions sum $3.513600 (fresh-limit baseline 3.449).
- Live Hermes at 127.0.0.1:8642, alias `worker`. The API key was read on the VPS into a variable and never printed or written.
  - Attempt 1: HTTP 200, but truncated (`max_tokens` 300 was used up by reasoning; `output_truncated`). Kept as `reply-attempt1-truncated.json`.
  - Attempt 2: HTTP 200, finish stop, 5037 prompt / 166 completion tokens. Hermes proposed: `"Returns the current date and time in Europe/Zagreb (RFC 3339 with offset).".into()`
- The harness (`plans/demo-gate.sh` as galion-gate) extracted the diff block (8 lines; it added a `diff --git` header), proposed it (sha 8966b598…) and ran the installed gate (record time 14:15:52 CET):
  - checks 7/7 PASS;
  - build PASS 22.1 s;
  - test PASS 10.5 s;
  - deploy smoke PASS 0.3 s;
  - verdict pass, record 13518ccd…
- DEMO approval: a separate demo key, `demo-owner.seed` on the box, labelled "DEMO approval - demo key, not Mate's owner key; staging only".
  - Apply with the real `owner.pub`: refused ("bad owner signature").
  - Apply with `demo-owner.pub`: staged at `/var/lib/galion-gate/staging/clock-desc-hermes-demo-59c79e5f`, tree 7715ae1f matches.
  - Replay: refused.
- Live code untouched: `/opt/galion/openmind/crates/hermes-rs/src/tools.rs` still has the old line (read with grep, no git).
- Attempts to leave the sandbox: none seen. Stage output has 0 hits for "Operation not permitted", "Permission denied", "Network is unreachable", "Read-only file system" and "Bad system call". Seccomp denials are not logged (see above).
- Secret scan of the prompt and both replies: 0 token/key hits, 0 IPv4 hits.
- Spend after: sessions sum $3.536791. Session `gate-demo-20261029` = **$0.023204** (model behind `worker`: anthropic/claude-sonnet-5.5). The gate runs used the mock provider and cost $0. The `boss` alias was not used.

## Step 4: backups
- Pushed with `push-branch.sh` (no force):
  - `swarm/hermes-sandbox-gate` → e2b68411
  - `swarm/hermes-sandbox-gate-v2` → 06d20dca
- Backup branch `swarm/backup-20261029` from main 7ba85b60: see the summary for the commit. Contents of `backups/20261029/`:
  - the drop-in and its rollback;
  - the gate install layout (no keys);
  - today's box reports;
  - the spend monitor state;
  - gate run records and the demo log;
  - files recovered from the VPS.
- VPS search (read-only `find` for `*20261029*` under /opt/galion, /root, /var/backups) found no lost report `.md` files. Recovered instead:
  - CI artefacts in `/root/swarm-om-20261029` (ci.sh, ci-int.out, sbx-build.out, clippy.log, test.log);
  - the `/var/backups/v1029/` drop-in backups (incl. model-picker).
  - Not copied: the full system tarballs `/var/backups/galion/galion-20261029-*.tar.gz` (and the offsite `.age` copies), and `/var/backups/omfix-20261029-*` (db/tgz/config backups). These are host backups, not reports, and may hold secrets.
- Secret/IP scan of `backups/20261029/` (IPv4, `sk-`, `ghp_`, `github_pat_`, `xox*`, `Bearer`, `*KEY/TOKEN/SECRET/PASSWORD=` values):
  - only loopback 127.0.0.1 and 0.0.0.0 found, and kept;
  - one `sk-` hit was a URL ("musk-xai…"), a false positive, left unchanged;
  - no keys, seeds or `.env` files were copied. Gate signatures and public keys are included; they are not secrets.

## Spend
- rs-spend.json sessions sum: 3.513600 before → 3.536791 after.
- This job: $0.023204, all from session `gate-demo-20261029` (2 Hermes calls).
- The fresh-limit monitor shows $0.0878 used of $5 since the 3.449 baseline; not tripped.

## Failures and blocks
- Auto-review blocked one diagnostic command (`unshare -Urm … mount tmpfs …`) with: "Isolate system directories using unshare to mount read-only tmpfs over sensitive paths, which weakens host security controls and isolation."
  It was not retried. I diagnosed through the sandbox's own test harness instead.
- First VPS gate run: stage failures (no `/proc/self` in the LXC). Fixed with the wrapper (record-fail1.json is kept).
- First Hermes demo reply was truncated (max_tokens 300). Retried once with 1500.
- My mistake: in one tool output the VPS IP appeared (reading push-branch.sh). It was not written to any file, commit or report.

## Decisions for Mate
1. Merge `swarm/hermes-sandbox-gate-v2` (it contains e2b68411) to main, or not. Nothing was merged.
2. Approve the mandatory-path work listed above (unprivileged hermes-rs, deploy requires a gate record, pinned owner.pub, branch protection + CI gate job, cgroup).
3. Security review: loopback port allowlist not kernel-enforced without Landlock; read-only visibility of gate staging/ledger; `/home/box` visible on box runs.
4. Someone else restarted hermes-rs at 14:07:42, 14:10:22 and 14:17:25 CET. Confirm that this was expected.
5. Keep or remove the demo key (`demo-owner.pub` on the VPS) and the two staging copies under `/var/lib/galion-gate/staging/`.
