<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# fmt-android-baseline (wave-4 slot 399), 2026-10-28 16:39-17:13 CET (+01:00)

Branch `swarm/fmt-android-baseline` off `swarm/real-ports-batch-2` 7c3de266.
Slot 381 reported two pre-existing baseline failures: `cargo fmt --check`
(galion-survival main.rs:251) and Android `cargo check`. Ports 22690-22699
(none needed: no listener was started by this slot).

## 1. Reproduction on 7c3de266

| check | result on 7c3de266 |
|---|---|
| `cargo fmt --all --check` | **pass** (exit 0). `crates/galion-survival` does not exist on this base. |
| `cargo check --workspace --target aarch64-linux-android` (plain, no env) | **fail**: `libsqlite3-sys` build script, cc-rs `failed to find tool "aarch64-linux-android-clang"` (then `aarch64-linux-android-ar`) |
| same for `armv7-linux-androideabi` with a zig CC | **fail**: 4 x E0308/E0277 in `crates/openmind-sandbox/src/treecopy.rs:77,82` (`st.st_mode` is u32, `libc::mode_t` is u16 on armv7 Android) |
| `scripts/ucpu-gate.sh check` (armv7 musl + aarch64-android, its own zig CC) | pass (16:41) |

F-399-1 (fmt): the galion-survival rustfmt hunk lives only on the
`swarm/survival-s47-install` b7fbbc78 lineage (also in `crashloop-r3-rebase`
657a9988 and `hermes-coding-dir-stage` 657a9988). It is already fixed by
`ea496d7e galion-survival: cargo fmt (slot 389)` on `swarm/survival-refresh-writer`
d25f8479. It cannot be fixed on this branch (the crate is not here); the fix to
carry is that one commit (a pure rustfmt split of one `if` chain, no logic).

## 2. Fix (no behaviour change on Linux targets)

- `openmind-sandbox/src/treecopy.rs`: new `mode(st) -> libc::mode_t` does
  `st.st_mode as libc::mode_t`; `kind`/`perm` use it. On x86_64/aarch64 musl,
  glibc and aarch64 Android the cast is the identity (`#[allow(clippy::unnecessary_cast)]`);
  on armv7 Android it narrows u32 -> u16, and every bit used (S_IFMT 0o170000,
  perm 0o1777) is in the low 16 bits.
- `.cargo/config.toml [env]`: `CC_`/`AR_` for `aarch64_linux_android` and
  `armv7_linux_androideabi` -> `scripts/android-cc.sh` / `scripts/android-ar.sh`
  (`relative = true`). Only these two Android triples read those names. Cargo
  `[env]` never overrides an exported variable, so the gate and NDK users keep theirs.
- `scripts/android-cc.sh`: NDK clang first (`<triple>[api]-clang` on PATH or in
  `$ANDROID_NDK_HOME`/`$ANDROID_NDK_ROOT`), else `zig cc` with musl headers
  (compile only, like the gate). `scripts/android-ar.sh`: llvm-ar, zig ar, ar.

Wrapper selection checks (box, 16:57): fake NDK `aarch64-linux-android24-clang`
on PATH -> used; `armv7a-linux-androideabi-clang` -> used; NDK via
`ANDROID_NDK_HOME` -> used; no NDK -> zig compiles `x.c` rc 0; unsupported
target -> exit 2; no zig and no NDK -> exit 127 with a clear message; llvm-ar preferred.

## 3. Proof the code is unchanged

Release `hermes-gateway` (the shipped binary that links openmind-sandbox via
hermes-tools), x86_64 musl, base vs new with `--remap-path-prefix` of each
worktree to `/src`: same size 2,313,184 B; `objdump -d` identical (only the
file-name header line differs); `cmp -l` = **5 bytes**, all `+9` (octal 72->103,
137->150, 251->262 x2, 255->266) = panic `Location` line numbers in treecopy.rs
shifted by the 9 added lines. aarch64 musl: same size 2,045,824 B.
`hermes-mock-model` byte-identical on both targets (bdbae376 / b756178f).

## 4. Tests (box, branch a7e3da8d)

| check | result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo clippy -p openmind-sandbox --all-targets --target armv7-linux-androideabi -- -D warnings` | pass |
| plain `cargo check --workspace --all-targets --locked --target aarch64-linux-android` | **pass** (was fail) |
| plain same for `armv7-linux-androideabi` | **pass** (was fail) |
| omnode (`edge/openmind-node`) plain check, both Android triples | pass |
| `ucpu-gate.sh grep check` with `UCPU_TIER2="armv7-unknown-linux-musleabihf aarch64-linux-android armv7-linux-androideabi"` | RESULT ok (16:55) |
| release build x86_64 + aarch64 musl (hermes-gateway, hermes-tools) | pass |
| `scripts/no-python.sh` | pass |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` base 7c3de266 | 2033 pass / 0 fail / 18 ignored |
| same on branch (`--no-fail-fast`) | 2032 / 1 / 18; the 1 = `room` `default_and_fixture_do_not_open_a_socket` flake (F-399-2) |
| test name sets base vs branch | identical, 2051 names |

First branch run also hit `galion-deploy systemd_backend` `bind 19394: AddrInUse`
(fixed port shared with another slot); it passed alone on both trees (1/0, 2.3 s).

F-399-2 (pre-existing flake): `room` lib test counts the process's own
`socket:[...]` fds while sibling tests run in parallel. `cargo test -p room --lib`
x5: branch 4 ok / 1 fail, base 4 ok / 1 fail. Not touched here.

## 5. VPS (sandbox_vps_only), 2026-10-28 17:00-17:13 CET

Throwaway `/var/tmp/sandbox-test-399-20261028170005` (0700), static x86_64 musl
openmind-sandbox test binaries for base and branch (examples demo/escape beside
them). Kernel 6.8.12-43-pve, LSMs lockdown,capability,yama,apparmor (no Landlock).

| suite | user | base | branch |
|---|---|---|---|
| lib (incl. 5 treecopy tests) | nobody | 9/0 | 9/0 |
| real_workdir_quota (GALION_REAL=1, treecopy copy-in/out q3) | nobody | 9/0 | 9/0 |
| real_escape (GALION_REAL=1) | nobody | 1/0 | 1/0 |
| sandbox (GALION_REAL_BASE set) | nobody | 30/1 | 30/1 (h2 `unmapped uid 65534`: nobody is the overflow uid, F-399-3) |
| sandbox | temp user sbx399t (uid 994, removed) | **31/0** | **31/0** |

Without `GALION_REAL_BASE` the sandbox suite fails 2/29 on both (tries the box
build path, EACCES). First VPS attempt hung: tests leave `sleep 3172/3181/20`
orphans that held the `$(...)` pipe (harness issue; the orphans are the suite's
own grandchild-escape probes). Re-run wrote to files and reaped only new
nobody/temp-user sleep processes. After: folder removed, temp user removed, 0
leftover processes, 0 listeners on 22690-22699. Nerve 127.0.0.1:8670 /state 200
(read-only). Cortex not observed by this slot. Nothing deployed, no unit touched.
