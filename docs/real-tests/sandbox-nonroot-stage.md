<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: hermes-rs non-root stage (slot 318, sandbox-nonroot-stage)

All times are CET (+01:00). The VPS is read-only for live state. Test runs were as uid/gid 65534
(`setpriv --reuid=65534 --regid=65534 --clear-groups`, `env -i`, `umask 077`) in
`/var/tmp/sandbox-test-20261027-223234`, which was removed afterwards. Ports 21880–21882 were
on loopback only. No temp user, no cgroup, no unit change. The live config.yaml and state.db were
never printed or copied to disk. Only hashes and field names were compared.

## Box (22:28 CET)
`cargo-slot test -j 2 -p hermes-gateway --test nonroot_stage`: **7 passed, 0 failed**. The real
test is vacuous (SKIP) on the box. `staged_preset` passes 8/0 with the new WORKDIR_MB=512 assert.

## VPS
Static x86_64-musl release `hermes-gateway` (sha256 43dbd6f34f21e808…) and the release test
binaries `nonroot_stage` and `staged_preset`.
Baseline at 22:33:20: hermes-rs MainPID 255539, NRestarts 0. nerve `/state` 200, hermes `:8642/health` 200.

### `nonroot_stage` run 1 at 22:33:20: 7 passed, 0 failed, 0.16 s
```
RUN as Uid: 65534 65534 65534 65534 Umask: 0077
HOST config.yaml stat: PermissionDenied; state.db open: PermissionDenied
HAZARD A: bare User= check-config == defaults: true (default_model=anthropic/claude-opus-5.5)
HAZARD B: exit=Some(512) after 22 ms, /health=0, stderr: ... hermes-gateway session store error:
          /opt/galion/hermes/state.db: unable to open database file
STAGED check-config default_model=nonroot-318-model
STAGED up in 50 ms on 21882
STAGED /v1/models=200 legacy session hnr318-385530=200 chat=200 no-auth models=401
STATE rs-state.db uid=65534 mode=600
STATE rs-spend.json uid=65534 mode=600
```
Runs 2 and 3 (22:34:03): 7/0 each.

### Live config hand-over vs root (22:33:53)
I compared the `--check-config` line from root with the live binary `hermes-gateway-b7fbbc78`
reading config.yaml directly. The other side was uid 65534 getting the same bytes on a pipe from
root (the way LoadCredential hands them over; no copy on disk).
| run | sha256[0..16] | equal to root |
|---|---|---|
| root, live binary | 5b19ed346931955b (1 route, 4 aliases) | — |
| uid 65534 bare (`User=` only), live binary and branch binary | 11cf967d8bd739b8 (`routes=[] aliases=[]`) | **no** |
| uid 65534 with the config handed over, live binary and branch binary | 5b19ed346931955b | **yes** |

The first try with `HERMES_RS_CONFIG=/dev/stdin` straight on a root-owned pipe failed with EACCES.
A pipe owned by the service user worked, and systemd's credentials dir is owned by that user too.

### `staged_preset` as uid 65534 (22:34:02): 8 passed, 0 failed, 0.61 s
Fs, Git and Cargo all `Exited(0)` and `Masked{6,0}`. `LINKENV=HIDDEN`, `OPTGALION=0`, `CANARY=HIDDEN`,
`PIDDIRS=0`. Control (no hide) `CANARY=READABLE`. WDQ 16 MiB: `WROTE=64` and the host got 16777216 B.
`staged_dropin_sets_the_required_keys` now asserts `HERMES_RS_CODING_WORKDIR_MB=512` (it was a FINDING in slot 305).

### `systemd-analyze verify` (systemd 255.4, read-only, copies in the test dir)
The live `hermes-rs.service`, plus 50-rust-bin, 60-nerve-url, 70-research-budget and 90-deploy-t291u, plus `80-nonroot.conf`: **rc 0, no output**.
The same with `User=nobody`: `80-nonroot.conf:28: Special user nobody configured, this is not safe!`, rc 0. This shows the drop-in was really parsed.

### After (22:34:35)
0 uid-65534 test processes, 0 `sandbox-test` mounts, folder removed, ports 21880–21889 free.
hermes-rs MainPID 255539, NRestarts 0 (unchanged). nerve `/state` 200 (cycle 25, `hermes-rs:healthy`).
hermes 200. link.env is still 666 and `/var/lib/hermes-rs` is still root 700 (untouched).

## Gates (box, 22:45 CET)
`cargo fmt --check` passes, and so does `cargo clippy -p hermes-gateway -p hermes-tools --all-targets -D warnings`.
`cargo test -p hermes-gateway -p hermes-rs` gives 78 passed, 0 failed (real_* gated tests vacuous), and `staged_preset` gives 8/0. `scripts/no-python.sh` passes.
x86_64 musl release builds. aarch64 musl release, armv7-musleabihf and aarch64-android `check --tests` pass with
`--no-default-features --features coding`. The default `sqlite` feature cannot cross-build on the box
tonight: there is no `aarch64-linux-musl-gcc` / `arm-linux-musleabihf-gcc` for libsqlite3-sys, and
zig-cc was not restored (D3 stopgap; this was already the case before this branch). This branch changes no `src/`.
