<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: realtest-real-env-guard (wave 4 slot 400)

Branch `swarm/realtest-real-env-guard` off `swarm/real-ports-batch-3` 26a526c1.
All times Europe/Zagreb, CET (+01:00). Run 2026-10-28 16:44-17:00 CET.

## Problem (F-383-10)

`GALION_REAL` was set in the shared box shell env, so slot 383's plain
`cargo test --workspace` (2026-10-28 11:40-13:08 CET) ran every
`GALION_REAL` test, including `galion-realtest` `real_sh_sandbox`, ON THE BOX.
That breaks the standing rule sandbox_vps_only. A single ambient variable was
the only thing between a workspace run and a sandbox run.

## Fix

New zero-dependency crate `crates/vps-sandbox-gate`. `sandbox_real(name)`
returns true only when ALL of these hold:

| # | Check | If it fails |
|---|---|---|
| 1 | `GALION_REAL=1` | `SKIP <test>: set GALION_REAL=1 GALION_REAL_SANDBOX=1 on the VPS` |
| 2 | `GALION_REAL_SANDBOX=1` (explicit per-run opt-in; never put it in a shared profile) | `SKIP <test>: GALION_REAL=1 is set but GALION_REAL_SANDBOX=1 is not ... (sandbox_vps_only, F-383-10)` |
| 3 | host check: `/opt/galion` and `/etc/galion` are dirs AND `/workspace/swarm` does not exist | `REFUSED <test>: not the VPS (vps_markers=.. box_marker=..) ... Nothing was run.` |

There is no override for check 3. Skips and refusals pass (exit 0), so an
ambient variable can never turn a workspace run red or into a sandbox run.

Gated tests (every GALION_REAL test that runs the sandbox):

| Crate | Test | Before |
|---|---|---|
| openmind-sandbox | `tests/real_escape.rs` `real_escape` | GALION_REAL=1 only |
| openmind-sandbox | `tests/real_workdir_quota.rs` q1-q9 | GALION_REAL=1 only |
| galion-realtest | `tests/real_sh_sandbox.rs` | GALION_REAL=1 only (doc said "Box only") |
| hermes-tools | `tests/real_ticket.rs` (sandboxed tools run cargo) | GALION_REAL=1 only |
| hermes-tools | `tests/sandboxed.rs` `real_default_512_mib_cap_stops_fs_write` | GALION_REAL=1 only |

`real_sh_sandbox` now also passes `GALION_REAL_SANDBOX=1` to the inner
`real_escape` run (otherwise the inner suite would skip).

Regression lint: `crates/vps-sandbox-gate/tests/every_sandbox_real_test_is_gated.rs`
runs in every workspace test. It scans `crates/*/tests` and `edge/*/tests` for
files that mention the `GALION_REAL` switch and run the sandbox
(`openmind_sandbox`, `hermes_tools::`, `real_escape`) and fails if one does not
call `vps_sandbox_gate::sandbox_real`. Today: 5 files checked, 0 missing.
Control: with the base (26a526c1) versions of real_escape.rs and
real_sh_sandbox.rs the lint FAILS naming exactly those two files.

## How to run the sandbox suites now (VPS, non-root, throwaway dir)

```
D=/var/tmp/sandbox-test-<ts>; mkdir -p $D/deps $D/examples $D/w; chown nobody $D/w
# copy deps/real_escape-*, deps/real_workdir_quota-*, examples/escape (static x86_64 musl from the box)
sudo -n -u nobody env -i PATH=/usr/bin:/bin HOME=$D/w \
  GALION_REAL=1 GALION_REAL_SANDBOX=1 GALION_REAL_BASE=$D/w \
  $D/deps/real_workdir_quota-<hash> --ignored --test-threads 1
rm -rf $D
```

## Evidence

Binaries (static x86_64 musl release, built on the box from 040329f4):
`real_escape-6ed08ddef62fc7f4` sha256 `b89e396678d26b60...`,
`real_workdir_quota-7f9efceb73515e45` `f1ece273ecf9357e...`,
`examples/escape` `bec248b9a94b4c3d...`.

### Box, 16:44:58-16:45:06 CET (sandbox never started)

| Env | real_escape | real_workdir_quota | Time |
|---|---|---|---|
| `GALION_REAL=1` (the ambient case) | 1/1 ok, `SKIP real_escape: GALION_REAL=1 is set but GALION_REAL_SANDBOX=1 is not` | 9/9 ok, 9 SKIP lines | 0.00 s each |
| `GALION_REAL=1 GALION_REAL_SANDBOX=1` | 1/1 ok, `REFUSED real_escape: not the VPS (vps_markers=false box_marker=true) ... Nothing was run.` | 9/9 ok, 9 REFUSED lines | 0.00 s each |

Box, 16:46:31 CET, debug test bins, same two envs:
`real_sh_sandbox` SKIP / REFUSED (no `build:` line, i.e. it did not even
start the nested cargo build); `real_ticket` SKIP / REFUSED;
`real_default_512_mib_cap_stops_fs_write` SKIP / REFUSED. All "ok", 0.00 s.

Side-effect check before and after every box run: `/sys/fs/cgroup/om-sandbox*`
0 -> 0, `/tmp/*om-sandbox-suid*` 0 -> 0, `target/test-out` entries 0 -> 0,
`$CARGO_TARGET_DIR/real-bins` absent -> absent, `/tmp/hermes-tools-real-*`
0 -> 0.

### VPS (sandbox_vps_only), 16:45:50-16:45:54 CET, uid nobody, `/var/tmp/sandbox-test-20261028-164521`

| Env | real_escape | real_workdir_quota |
|---|---|---|
| none | SKIP, 1/1 ok, 0.00 s | 9 SKIP, 9/9 ok, 0.00 s |
| `GALION_REAL=1` only | SKIP (no opt-in), 1/1 ok, 0.00 s | 9 SKIP, 9/9 ok, 0.00 s |
| `GALION_REAL=1 GALION_REAL_SANDBOX=1` | **ran**: 1 passed, 1.27 s; tcp/udp/tcp-internet/ptrace/kill/seebystander/mount/remount-root/read/write CONTAINED | **ran**: 9 passed, 3.05 s; q1 host_bytes=268435456 (= 256 MiB cap), q2 67108864, q4 fails closed ENOSPC, q8 stop=1023, q9 NOTE skipped (no delegated cgroup parent) |

real_escape printed one `RESULT procpeek ESCAPED pid_dirs=45 ...` line next to
`procpeek CONTAINED pid_dirs=0`; the test still passed, so this is the suite's
known /proc-mask measurement (S10 "MEASURED" in docs/real-tests/sandbox-workdir-quota.md),
not caused by this change. No setuid helper and no delegated cgroup were used
(no sudo for nobody), so those checks were skipped as designed.

After: dir removed (`ls` -> No such file), 0 processes of uid nobody, 0
`om-sandbox*` cgroups. Live hermes-rs MainPID 255539 NRestarts 0 before and
after. Nerve `GET 127.0.0.1:8670/state` 200 (galion-nerve.service active);
cortex not checked (not live for this row). No live file, unit or permission
changed.

### Not run

A full `cargo test --workspace` with `GALION_REAL=1` exported was NOT run: it
would start the ~120 other GALION_REAL tests on their default ports alongside
other running slots (the collision F-383 already hit). The proof is the five
gated tests above plus the lint that keeps every future sandbox real test
behind the gate. The ordinary workspace gate (`env -u GALION_REAL`) is below.
