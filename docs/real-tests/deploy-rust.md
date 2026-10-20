<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: deploy-rust (swarm slot 39), Rust galion-deploy

- Branch `swarm/deploy-rust`, based on `swarm/deploy-sim` c5ab8f9 (which is on swarm/tuner-cutover 5fd1611, base main 13a296c).
- Times are Europe/Zagreb, CEST (+02:00), 2026-10-20.
- **Nothing was installed, copied, written, restarted or POSTed on the VPS.** The only VPS access was one batch of read-only GETs plus `systemctl is-active` (section 5).
- Ports used on the box: 19390-19394 and 19399 (dead port), slot 39 only. After every run, `ss` showed 0 listeners in 19390-19399 and no process from `/workspace/swarm/sim/deploy-rust` was left.

## 1. What replaced what

| before (swarm/deploy-sim) | now |
|---|---|
| `deploy/foundation/galion-deploy` (bash, pluggable) | `crates/galion-deploy`, a std-only Rust binary, 577 KB static x86_64 musl. The bash text is kept, not installed and not run, at `docs/reference/galion-deploy.bash` |
| `scripts/real/galion-deploy-equiv.sh` (bash shims) | `crates/galion-deploy/tests/systemd_backend.rs`. The `sm_shim` example plays `systemctl`/`journalctl`/`ctl`/`galion-backup.sh`, and each scenario's exact call sequence is asserted |
| `scripts/real/deploy-sim.sh` | `crates/galion-deploy/tests/real_rehearsal.rs` + `tests/support/mod.rs` (GALION_REAL=1) |
| `scripts/real/deploy-sim-build.sh` | `crates/galion-deploy/tests/real_build.rs` (GALION_REAL=1 verifies the 11 pinned sha256s; GALION_REAL_BUILD=1 builds them) |
| `sim/deploy-sim/tests/real_deploy.rs` (ran the bash script) | removed; `rehearsal_tuner` covers it |

`simsup` (sim/deploy-sim) is unchanged, and its control verbs are the direct backend.

## 2. Results

```
GALION_REAL=1 nice -n 10 cargo test -j 2 --release -p galion-deploy --test real_rehearsal -- --nocapture --test-threads 4
```

All four ran in parallel, 201 s.

| rehearsal | deploy-sim (bash) run 5 | **deploy-rust (Rust)** | full log |
|---|---|---|---|
| tuner | 21 pass, 0 fail | **24 pass, 0 fail** | `docs/real-tests/deploy-rust/tuner.log` |
| nerve | 18 pass, 0 fail | **19 pass, 0 fail** | `docs/real-tests/deploy-rust/nerve.log` |
| room | 5 pass, 0 fail | **7 pass, 0 fail** | `docs/real-tests/deploy-rust/room.log` |
| desk | 8 pass, 0 fail | **8 pass, 0 fail** | `docs/real-tests/deploy-rust/desk.log` |

The bash checks that proved a finding now prove its fix:

| finding | bash rehearsal result | Rust rehearsal result |
|---|---|---|
| F1 tuner hang A (sim-hang) | deploy never returns, killed by the outer timeout (124) | exit 3 after 29.3 s: the `--version` step is bounded at 5 s, then install, health fails, auto rollback. No orphan process |
| F1 tuner hang C (unit down, L2 to the old tuner) | hung while `tuner-<stamp>.bin --version` served :19330 outside the unit (124) | exit 0 in 9.1 s. The stray `--version` (pid logged) held the port for at most 5 s, then its process group was killed, and the unit took over |
| F1 desk down, forward deploy | hung (124) | exit 0, new sha, healthy. Log: `--version did not exit within 5s; killed its process group` |
| F1 nerve sim-hang | hung (124) | exit 3, service restored |
| F2 nerve, 5th start in the 600 s window | rollback restart refused, deploy died, exit 1, unit `failed` | exit 3, unit `active`, healthy. The default reset-failed before the forward restart empties the window, so the broken build's crash loop and the rollback restart fit |
| F2 opt-out `GALION_DEPLOY_RESET_FAILED=0`, full window | (same trap) | trap reproduced (`Start request repeated too quickly`, `rollback restart refused`) and reported as **exit 4** + `ROLLBACK ALSO UNHEALTHY`, not the ambiguous 1. Recovery: reset-failed + restart, 38 ms |
| F2 tuner L1 ×5 (broken, truncated ×4, back to back, no manual reset-failed between runs) | trap in 3 of 8 attempts across runs 2-5 | 5 of 5 exit 3, unit active, healthy |
| F3 exit codes | 1 for everything | tuner saw 0, 1 (lock), 3, 5 and 137 (SIGKILL); nerve 0, 3, 4, 5; room 0, 3 |
| F4 nerve broken deploy judged on `/health` | `ROLLBACK ALSO UNHEALTHY` although the old nerve served `/healthz` | exit 3. Log: `health failed, rolling back … rollback_health=http://127.0.0.1:19391/healthz` |
| F5 room first install | `cannot resolve installed binary for room` (exit 1) | broken first install: exit 3, binary removed, unit inactive, 0 listeners. Real first install: exit 0 in 4.1 s, healthy, no backup |
| F6 SIGTERM, SIGHUP, SIGINT after the install (tuner ×3, nerve, desk) | new binary left in place, no rollback | exit 5 every time, the binary that was there before is back, healthy, journal gone |
| F6 SIGKILL (tuner) | (same) | journal survives with `phase=verify`. A second deploy during the run was refused (exit 1, `another galion-deploy (pid …)`). `galion-deploy --recover tuner` exited 3, old sha back, healthy |

Unchanged from deploy-sim and still passing: 3.1 artefact sha256, the 3.2 `--version`/`--check` preflight, the running exe's sha, `/` = 200 / 2356 B and `/nope` = 404, **21/21 tuner routes byte-identical to the live fixtures**, L2 and L3 rollbacks, nerve data-dir compatibility both ways, room health checks (ok, `{"objects":[`, 404 for /etc/passwd, loopback only), the desk's 22 `/desk` keys, and the desk manual swap and step-13 rollback.

### Downtime (probe: one GET every 2 ms, or 5 ms in the long scenarios)

| scenario | tuner | nerve | room | desk |
|---|---|---|---|---|
| planned swap via galion-deploy | 2 ms | 4 ms (final swap 2 ms) | 2 ms (redeploy) | 2 ms |
| manual swap / step 13 (no galion-deploy) | | | | 0 ms / 0 ms (no refused try) |
| rollback L2 (galion-deploy with the backup) | 2 ms | 3 ms | | 2 ms |
| rollback L3 (install + mv + restart) | 2 ms | 2 ms | | |
| rollback L1 (automatic, broken / truncated / hang) | 20,183–20,243 ms | 20,183 ms (20,239 ms with the full window) | 20,201 ms | 20,180 ms |

These match deploy-sim (2–5 ms planned, about 20.2 s for L1). The L1 figure is the 20 × 1 s health window, which is kept on purpose because the bash version and the docs promise it. See open issue 2.

## 3. systemd backend (no systemd on the box: shimmed)

```
$ cargo test -p galion-deploy --test systemd_backend -- --nocapture
  OK  healthy: exit 0, 5 calls
  OK  healthy-unit-suffix: exit 0, 5 calls
  OK  broken-rolls-back: exit 3, 28 calls
  OK  rollback-also-bad: exit 4, 47 calls
  OK  restart-refused: exit 4, 11 calls (old binary restored)
  OK  unit-only-health: exit 0, 10 calls
  OK  unknown-unit: exit 1, 1 call
  OK  not-elf: exit 1, 2 calls
  OK  missing-new: exit 1, 2 calls
  OK  usage: exit 2, 0 calls
  OK  first-install: exit 0; broken first install: exit 3, binary removed, unit stopped
  OK  sigterm-in-health-window: exit 5, old binary back
  OK  sigkill: journal phase=verify kept; second deploy refused (lock, exit 1); --recover exit 3, old binary back; again exit 0
  OK  data-backup: runs when executable and not skipped
  OK  direct backend: broken -> exit 3; missing CTL / unknown SM -> exit 1
```

The scenarios are the ten of `galion-deploy-equiv.sh` plus five new ones. Each one asserts the exact ordered `systemctl`/`journalctl` calls. These are the original calls, with three intended differences: `reset-failed` before each `restart` (F2); no `sleep`/`curl` subprocesses (both run in-process now); and in restart-refused, a retry plus a rollback. In that case bash died with the new binary installed. Each scenario also checks the backup name format, modes (0700 dirs, 0755 target, `cp -p` mode on the backup), the log lines and the exit codes. Plus 28 unit tests (SHA-256 NIST vectors, DST edges, journal round trip and lock, ExecStart parsing, HTTP status parsing and timeouts, the state machine against an in-memory backend, signal paths, pruning).

## 4. Builds and checks

- `cargo fmt --check`, `cargo clippy -p galion-deploy --all-targets -- -D warnings`: clean.
- `cargo build --release --target x86_64-unknown-linux-musl` (577,144 B, static-pie) and `aarch64-unknown-linux-musl` (517,464 B): ok. `cargo check --all-targets` for `aarch64-linux-android` and `armv7-unknown-linux-musleabihf`: ok.
- `GALION_REAL=1 cargo test --release --test real_build`: all 11 rehearsal binaries match their pins, and tuner-new is byte-identical to the swarm/tuner-cutover artefact. With `GALION_REAL_BUILD=1 GALION_REAL_BUILD_ONLY=simsup,sim-broken,sim-hang,tuner-new`, a fresh build from this tree and a fresh 5fd1611 worktree reproduced the same four sha256s.
- `sim/deploy-sim`: `cargo check --tests` ok after removing `tests/real_deploy.rs`.

## 5. Read-only VPS probe (GET only, nothing written)

```
probed at 2026-10-20T23:19:00+02:00
http://127.0.0.1:8670/healthz 200      <- galion-nerve table entry (F4) is right
http://127.0.0.1:8670/health 404       <- the live nerve has no /health
http://127.0.0.1:8650/health 200
http://127.0.0.1:8646/health 200
http://127.0.0.1:4020/health 000       <- room not installed (table entry is for the staged unit)
http://127.0.0.1:8420/health 200
http://127.0.0.1:8642/health 200
http://127.0.0.1:8660/status.json 200
systemctl is-active galion-nerve tuner galion-desk: active active active
```

Every health-table URL the live services have answers 200.

## 6. Re-run

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/deploy-rust CARGO_INCREMENTAL=0
cargo test -p galion-deploy                                                    # unit + systemd backend
GALION_REAL=1 cargo test --release -p galion-deploy --test real_build -- --nocapture
GALION_REAL=1 cargo test --release -p galion-deploy --test real_rehearsal -- --nocapture --test-threads 4
```

Logs land in `/workspace/swarm/sim/deploy-rust/runs/`.
