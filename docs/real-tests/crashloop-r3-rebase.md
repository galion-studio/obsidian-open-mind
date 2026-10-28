<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: crashloop-r3-rebase (wave-4 slot 381)

All times Europe/Zagreb CET (+01:00). No secrets. Ports 22510-22512, loopback only.

## What was ported

`swarm/hermes-rs-crashloop-r3` d76b70df (slot 355, `--check-config` coding
validation, `HERMES_RS_CODING_ON_ERROR=disable`, deny/RW hints, staged drop-ins)
cherry-picked onto live **b7fbbc78** (`/opt/galion/rust/bin/hermes-gateway-b7fbbc78`).
`git merge-tree` of the whole slot-355 branch conflicts because it also carries the
hermes-offhost commits 1e0f427b..6845c350 (api.rs etc.), which are not on b7fbbc78.
Only d76b70df is ported. The single conflict was `docs/hermes-tools.md` (base
`WORKDIR_MB`/`WORKDIR_INODES` rows vs new `ON_ERROR` row): kept all three. `main.rs`
and `config.rs` merged cleanly. Test ports moved 22250/22251 -> 22510/22511.

## Box, 2026-10-28 10:38:48 CET (uid 1000)

`GALION_REAL=1 HERMES_GW_BASE_BIN=<b7fbbc78 release musl> cargo test -p hermes-gateway --test real_crashloop_r3`: 1 passed.

| case | result |
|---|---|
| R1 start, worktree `/var/lib/dpkg` | exit 2 in 20 ms, deny-overlap + ReadWritePaths hint, no bind; restart x3 -> [2,2,2] |
| R2 `--check-config` same config, new | exit 2, `coding=error` |
| R2-before, b7fbbc78 build | exit 0, no coding line (blind spot reproduced on the live base) |
| R3 state dir not writable (0500) | exit 2, `not writable: Permission denied` + RW hint |
| R4 good config / no coding env | exit 0 `coding=on ...` (nothing created) / exit 0 `coding=off` |
| R5 bad config + `ON_ERROR=disable`, `--mock` :22511 | up in 151 ms, /health 200, /v1/models 200, `coding tools OFF, gateway continues` |

## VPS throwaway as nobody (setpriv uid/gid 65534), 2026-10-28 10:41:37-10:42:33 CET

Folders `/var/tmp/sandbox-test-381-20261028104046` and `-20261028104214` (run dir
nobody 0700), static x86_64 musl test binaries copied in, removed afterwards.
VPS kernel: no Landlock LSM, userns yes; git/dd/install present.

| suite | result |
|---|---|
| hermes-tools lib / guard / staged_dropin / workdir_quota | 3/0, 6/0, 4/0, 5/0 |
| hermes-tools sandboxed, no override | 2 passed / 8 failed (`preflight` refuses: no Landlock). **Same 2/8 on the b7fbbc78 build** -> pre-existing, matches slot 335 (xtask auto excludes hermes-tools when landlock=no) |
| hermes-tools sandboxed, `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` | new 10/0 (1.39 s), base 10/0 (1.04 s) |
| real_crashloop_r3 (GALION_REAL=1, new + base bins) | 1 passed, R1-R5 as on the box (R1 exit 2 in 1 ms, R5 up in 151 ms on :22511, base `--check-config` exit 0) |

`--check-config` as nobody, `HERMES_RS_CODING_BRANCH=hermes/ticket`, port 22512 (never bound):

| binary | worktree | result |
|---|---|---|
| live `hermes-gateway-b7fbbc78` (run read-only from its path) | `/var/lib/hermes-rs/coding-worktree` (live 19:45 shape) | exit 0, no coding line |
| live b7fbbc78 | `/srv/hermes-coding/worktree` (staged shape) | exit 0, no coding line |
| new | `/var/lib/hermes-rs/coding-worktree` | exit 2, `coding=error`, `Permission denied` (nobody cannot traverse /var/lib/hermes-rs; as the service user the deny-overlap path applies, see R1) |
| new | `/srv/hermes-coding/worktree` | exit 2, `coding=error`, `No such file or directory`: the staged path does not exist on the VPS yet, so runbook step 1 (`install -d` + clone) must come first |

Live after both runs: hermes-rs MainPID 255539, NRestarts 0, active; :8642 /health 200;
nerve :8670 /state 200. 0 nobody processes from the folders, 0 listeners on 22510-22519,
folders and uploads removed. Nothing installed, no unit touched.

## Other checks (box)

- `cargo test -p hermes-rs -p hermes-gateway`: hermes-rs 48+2+16+1+1, gateway real_* skip-gated pass, real_crashloop_r3 above.
- `cargo test -p hermes-tools --lib --test guard --test staged_dropin`: 3/0, 6/0, 4/0 (sandboxed/workdir_quota only on the VPS, sandbox_vps_only).
- `cargo clippy --workspace --all-targets -D warnings` ok; `-p hermes-gateway --no-default-features` ok.
- `cargo fmt -p hermes-gateway -p hermes-tools -p hermes-rs --check` ok; workspace `fmt --check` fails only on
  `crates/galion-survival/src/main.rs:251`, identical on b7fbbc78 (pre-existing, not touched).
- musl x86_64 + aarch64 release build ok; `cargo check` armv7-musleabihf ok, aarch64-android ok with
  `--no-default-features` (default fails in libsqlite3-sys build script, same on b7fbbc78);
  armv7-androideabi fails in `openmind-sandbox/src/treecopy.rs:77-82` (pre-existing, untouched).
- `scripts/no-python.sh` ok.
