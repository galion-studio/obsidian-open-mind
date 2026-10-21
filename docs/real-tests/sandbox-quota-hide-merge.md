<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# sandbox-quota-hide-merge: work dir cap + /opt/galion hide together, VPS proof

Wave-4 slot 176. Branch `swarm/sandbox-quota-hide-merge`. Times are
Europe/Zagreb, CEST +02:00.

## Merge
- First parent `swarm/hermes-tools-quota` 2b9944e (includes
  `swarm/sandbox-workdir-quota` 28f6c1d), second parent
  `swarm/sandbox-hide-galion` ad0ed9b. Merge base 334c633.
- Merge commit 285d8bb (made 08:55 by the first slot-176 agent, which was
  stopped by the request limit at 08:56). The retry agent checked it from
  09:49 before using it:
  `git merge-tree 2b9944e ad0ed9b` shows exactly the 5 known text conflicts
  (deploy/sandbox/presets/{build,net,read-only}-tool.toml,
  crates/openmind-sandbox/tests/preset.rs, docs/sandbox-presets.md). Auto-merged:
  hermes-tools/src/config.rs (doc comment), openmind-sandbox/src/preset.rs,
  tools/sandbox-real-problems/src/runner.rs. No conflict markers are left.
- How each conflict was resolved: the preset TOMLs keep both sides.
  `hide_paths = ["/root", "/home", "/run/secrets", "/opt/galion"]` stays, and
  so do `workdir_tmpfs_bytes` 512 MiB (build-tool) and 64 MiB (net and
  read-only), with `workdir_tmpfs_inodes = 0`. `tests/preset.rs` keeps both
  new tests, `workdir_tmpfs_cap_matches_toml_and_is_on_for_every_preset` and
  `tool_presets_hide_opt_galion_and_match_toml`, plus both sides' asserts.
  `docs/sandbox-presets.md` keeps the work dir cap rule and the new
  `/opt/galion` hide rule.
- Compared with each parent: vs 2b9944e, the only changes are the
  `/opt/galion` hide, the S8 runner and the hide doc. Vs ad0ed9b, the only
  changes are the work dir cap fields and tests. Cargo.toml and Cargo.lock
  match 2b9944e, so there are no new dependencies.
- hermes-tools coding tools start from `Preset::*.policy()` and only extend
  `hide_paths`, so they now hide `/opt/galion` too (the worktree stays visible
  when it is under it, reported in `hide_home`).

## Box tests (no sandbox runs on the box)
- `cargo fmt --check` OK (workspace and tools/sandbox-real-problems).
- `cargo clippy --workspace --all-targets -- -D warnings` OK; runner clippy
  (musl release, all targets) OK.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  **957 passed, 0 failed, 3 ignored** (205 test binaries).
- `cargo test -p openmind-sandbox --test preset`: **3 passed** (pure policy:
  TOML files vs Rust presets; it asserts that no process is spawned). It ran on
  the box because it reads the TOML files from the source tree, which the VPS
  does not have. On the VPS the same binary failed 3/3 with ENOENT on
  those paths, which is an environment limit and not a code failure.
- x86_64 musl release (all VPS binaries static-pie). aarch64 musl release
  build of openmind-sandbox + hermes-tools OK (rust-lld). `cargo check`
  armv7-unknown-linux-musleabihf and aarch64-linux-android for both crates OK.
  Note: the android check of the full workspace fails in `libsqlite3-sys`,
  which needs an NDK C compiler. That was already the case before this merge;
  Cargo.lock is unchanged.
- `scripts/no-python.sh`: ok.

## VPS run (2026-10-21 09:53:40 - 09:56:40 +02:00), sandbox_vps_only
- Throwaway dir `/var/tmp/sandbox-test-20261021-095340`: `bin/` is root 0755,
  `data/`, `w/`, `t/` and `tmp/` belong to the temp user. Temp system user `sbx176`
  (nologin, uid 995). Every run used
  `systemd-run --wait --pipe --collect --slice=sbxtest.slice -p User=sbx176 -p Group=sbx176 -p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10`.
  The test binaries that need a per-run cgroup ran through a 9-line wrapper.
  It moves itself into `<unit>/sup`, enables `+memory +pids +cpu`, and
  exports `GALION_REAL_CGROUP_PARENT`.
- Data prep as root, the same as the sandbox-real-problems run 3 prep:
  - The cloudflared journal for 2026-10-20 09:52:40 to 2026-10-21 09:52:40,
    masked by `srp mask` running as sbx176 (28,220 IPv4 and 2 IPv6 masked,
    18.9 MB).
  - `ref.txt` with 69 keys.
  - A copy of the openmind-memory DB + WAL. The sha256 prefixes were
    `0ff84fad0d753fd3` and `f50024d3b47a2837` before the copy, after it, for
    the copies, and after cleanup. The originals were never opened for writing.
  - 48 secret paths. Only the words ESCAPED and CONTAINED are printed.
  - A 32 MiB tmpfs on `w/s9c-tmpfs` and another on `small/` (for the
    hermes-tools D-WDQ3 case).
- Binaries: static x86_64 musl from merge 285d8bb. sha256 prefixes: srp
  `ea7424692f6bda1c`, sandbox suite `624224353340781f`, real_workdir_quota
  `afbe861473a316ec`, real_escape `b7fddb7fee1c3280`, escape example
  `3987d86efa290d9f`, hermes-tools sandboxed `75fd75f268be5e95`, guard
  `cf5d9896f7dc987a`, workdir_quota `711d9cf5689847ec`.

| Suite | Result |
|---|---|
| srp S1-S10 (09:54:34-09:54:56) | **SUMMARY 10 scenarios, 0 FAIL**: S1-S9 PASS, S10 MEASURED (known /proc gap) |
| real_workdir_quota (GALION_REAL=1, q1-q9, delegated cgroup) | **9 passed, 0 failed** (3.76 s) |
| openmind-sandbox suite (tests/sandbox.rs) | **26 passed, 0 failed** (5.02 s) |
| openmind-sandbox lib unit tests | 5 passed, 0 failed |
| hermes-tools sandboxed (GALION_REAL=1, HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1, small fs) | **10 passed, 0 failed** (2.25 s), including the real default 512 MiB cap and the D-WDQ3 partial copy back |
| hermes-tools guard / workdir_quota / real_ticket | 6/0, 5/0, 1 "passed" (= skip: needs GALION_REAL + toolchain = a cargo build on the VPS, not done) |
| real_escape (GALION_REAL=1, delegated cgroup) | 11 CONTAINED, **1 ESCAPED = `seebystander`** (host /proc stays visible because the LXC refuses a proc mount; this is the known S10 gap). **The same failure happens on base 2b9944e** (real_escape built from 2b9944e, run 09:56:21-09:56:25: the same single `seebystander ESCAPED`, 11 CONTAINED), so the merge did not cause it |

### S8 is PASS with the S9 caps kept (the row 176 acceptance)
```
HOST OK [S8] preset: shipped read-only-tool hides /opt/galion
HOST OK [S8] preset: 48 of 48 paths judged
HOST OK [S8] preset: 0 readable of 48 (0 non-template [], 0 .env.example templates); 47 contained, 1 hidden
NOTE [S8] baseline preset-before-fix: 11 readable of 48 (1 non-template [".../publish/obsidian/link.env"], 10 .env.example templates)
NOTE [S8] baseline nohide: 11 readable of 48 (same)
LAYERS ... workdir_quota=Enforced ... ro_root=Enforced | hide_home=Enforced | proc=NotEnforced(...) | landlock=NotEnforced("kernel has no landlock") ...
```
S9 with the shipped presets of the merge:
| Case | Result |
|---|---|
| a: RLIMIT_FSIZE 16 MiB | EFBIG, file is exactly 16,777,216 B, copied back |
| b: preset cap 64 MiB, 256 MiB asked | ENOSPC; the tool stopped at 63 MiB, the host got 66,220,032 B back, 124 ms (gap closed) |
| c: cap off, 32 MiB host tmpfs | ENOSPC at 31 MiB, layer reported NotEnforced |
| c2: 64 MiB cap over a 32 MiB host fs | stopped at the cap (63 MiB); copy back complete=false errno=28 |
| d: 256 MiB cap, 320 MiB asked | ENOSPC at 255 MiB, tmpfs used 268,435,456 B, 267,546,624 B copied back complete, memory.peak 258 MiB |
| e: 256 MiB cap under memory.max 128 MiB | Signaled(9), 3 OOM kills, host 0 MiB |
Other scenarios: S1 64 keys equal to root's grep counts. S2: 0 DB errors, and
the sandboxed and unsandboxed checker outputs are identical (67 lines). S3: a
crash loop killed at its wall limit, cgroup removed. S4: 3 OOM kills at
128 MiB. S5: 178 pids.max hits, peak 24. S6: hang + setsid daemon cleaned up.
S7: network contained. S10: 37 other-uid cmdlines readable (known gap,
unchanged).

### First srp attempt (09:54:07), harness error, kept for honesty
The first run used a slot-only slice `sbx176.slice`. The runner requires
`sbxtest.slice` ("NOTE not inside sbxtest.slice"), so S3-S6 failed with "no
delegated cgroup". S1, S2 and S7-S10 passed in that run, S8 included. The
re-run in `sbxtest.slice` (above) is the result that counts. The empty
`sbx176.slice` was stopped afterwards (0 processes).

### Cleanup (09:56:37-09:56:40, plus the slice stop at ~09:57)
No processes were left. Both tmpfs are unmounted, the dir is removed, and the
user and group are deleted. No sbx176 unit or cgroup is left. 0 srp/rp
processes and 0 mounts remain. The DB originals have unchanged sha256.

### Live system (read-only)
- Nerve `127.0.0.1:8670/state` was HTTP 200 before (09:53:31) and after the
  run (cycle 26, mode live). The cortex is not live yet, so the nerve was
  used. Unit MainPIDs and NRestarts are identical before and after for
  galion-nerve, galion-nerve-shadow, hermes-rs, openmind-memory, tuner,
  tuner-quick, cloudflared, link-watch-rs and galion-desk (all 0 restarts).
  The nerve shows openmind-memory and hermes-rs as "degrading", which is the
  restart count from the slot-180 deploy (09:45/09:46), not caused by this
  run. cloudflared is "failing-soon" (origin errors, known).
- VPS load average was about 7.9 before the run.
- Nothing was installed, enabled or restarted. /opt/galion/openmind was not
  touched.
