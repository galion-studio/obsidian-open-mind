<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: galion-deploy crash-loop fail-fast (wave 4, slot 121)

- Branch `swarm/galion-deploy-fastfail`, based on `swarm/deploy-rust` 787a54d.
- Times are Europe/Zagreb, CEST (+02:00), 2026-10-21.
- **Nothing was installed, written, restarted or POSTed on the VPS.** The only VPS access
  was one read-only `systemctl --version` + `systemctl show -p Restart -p RestartUSec -p
  NRestarts -p ActiveState -p StartLimit…` batch (section 4).
- Box ports: 19910 (tuner) and 19911 (galion-nerve), slot 121's range. The flag-off
  regression reused deploy-rust's own 19390-19399. Afterwards `ss` showed 0 listeners
  in either range.

## 1. What changed

Open issue 2 of `docs/real-tests/deploy-rust.md` said the automatic (L1) rollback of a
build that crashes at start costs the whole 20 × 1 s health window (~20.2 s of outage).
The fix is opt-in and changes nothing when the flag is unset:

`GALION_DEPLOY_FAILFAST=N` (N > 0): during the forward health window galion-deploy also
reads the unit every 250 ms (`systemctl show -p ActiveState -p SubState -p NRestarts`;
direct backend `ctl show`). It gives up and rolls back as soon as:

- **crashes ≥ N**, where crashes = `NRestarts` growth since the deploy's own restart, plus
  1 if `SubState=auto-restart` (a crash that systemd has not restarted yet). The baseline
  is read after the restart, so it does not matter whether systemd zeroes the counter on
  a manual restart (simsup does not: the logs show `nrestarts_base=9`, and so on).
- or the unit is `failed` on **two polls in a row**: `Restart=no`, or the start limit
  was hit, so nothing will restart it. One poll is not enough, because systemd ≥ 254
  passes through `failed` for an instant on the way to an automatic restart, and the
  VPS runs systemd 255.

Log lines added: `fail-fast armed … crash_limit=N nrestarts_base=… state=…/…` and
`crash loop: N crash(es) of the new build since the deploy restart (limit N); failing
fast … after_ms=…`. The documented `health failed, rolling back` line, the journal
dump, the rollback and the exit codes (3/4) are unchanged. The rollback's own health
check never fails fast. If `NRestarts` cannot be read, galion-deploy logs `fail-fast off:
cannot read NRestarts; using the full health window` and keeps the old window.

Code: `crates/galion-deploy/src/backend.rs` (`Manager::unit_state` with a default `None`,
`parse_unit_state`), `src/deploy.rs` (`FailFast`, `pause`, `failfast_arm`; `--help`),
`examples/sm_shim.rs` (`SHIM_NRESTARTS`). std only, no new dependency.

## 2. Rehearsal numbers (simsup, GALION_REAL=1)

```
GALION_REAL=1 nice -n 10 cargo-slot test -j 2 --release -p galion-deploy --test real_failfast -- --nocapture --test-threads 2
```

The run started at 2026-10-21T04:11:25+02:00 and took 127 s: **tuner 12 pass, 0 fail; nerve 5 pass, 0 fail**.
Full logs: `docs/real-tests/galion-deploy-fastfail/tuner.log`, `…/nerve.log`.
Units use the live `Restart=`/`RestartSec=`/start limit (section 4). Outage = longest
gap without a 2xx in a probe every 5 ms (2 ms for the swap). Deploy time includes the
3 s data-backup stand-in.

| unit (RestartSec) | new build | FAILFAST | exit | outage | deploy time |
|---|---|---|---|---|---|
| tuner (2 s) | sim-broken (exits 1 at start) | off | 3 | **20,192 ms** | 24.3 s |
| tuner | sim-broken | 1 | 3 | **2,118 ms** | 6.2 s |
| tuner | sim-broken | 2 | 3 | **4,161 ms** | 8.3 s |
| tuner | sim-broken | 3 | 3 | **6,218 ms** | 10.4 s |
| tuner | truncated ELF (exec fails, 203/EXEC) | off | 3 | 20,182 ms | 24.3 s |
| tuner | truncated ELF | 2 | 3 | **4,168 ms** | 8.3 s |
| tuner | sim-hang (alive, never binds) | 2 | 3 | 20,641 ms (no crash, so no help) | 29.8 s |
| tuner | tuner-new (good) | 1 | 0 | 2 ms | 4.2 s |
| galion-nerve (10 s) | sim-broken | off | 3 | 20,193 ms | 24.3 s |
| galion-nerve | sim-broken | 1 | 3 | **10,106 ms** | 14.2 s |
| galion-nerve | sim-broken | 2 | 3 | 20,352 ms (2 × RestartSec, no gain) | 24.5 s |

**Transient false alarm (the reason not to use N = 1).** A *good* tuner build whose
first start fails (port still held, released right after that crash), with the
automatic restart 2 s later succeeding:

| FAILFAST | result |
|---|---|
| 1 | exit 3 after 3.2 s: **the good build was rolled back** |
| 2 | exit 0 after 2.2 s: kept, healthy |

Read the table this way: on simsup a crash counts when the unit is restarted, so
outage ≈ N × RestartSec + ~0.1 s. simsup has no `SubState`. Under systemd the pending
`auto-restart` counts the first crash at once, so the bound there is ≈ (N − 1) ×
RestartSec + one 250 ms poll: tuner N=2 ≈ 2.3 s, nerve N=1 ≈ 0.3 s, nerve N=2 ≈ 10.3 s.
These systemd figures are *derived*, not measured. The box has no systemd (PID 1 is
tini), and the SubState path is exercised only through the shim (section 3).

### Flag off: no regression

`GALION_REAL=1 cargo-slot test --release -p galion-deploy --test real_rehearsal -- --test-threads 4`
started 2026-10-21T04:16:06+02:00: **tuner 24/0, nerve 19/0, desk 8/0**. room failed at
setup on that first run only because commit 5bb27e1 (swarm/room's staged unit) was
missing from this clone. After fetching it from /workspace/swarm/wt/room it gave **room
7/0** (04:22). These are the same counts as deploy-rust 787a54d, and every L1 outage
is still 20,174–20,223 ms.

## 3. Unit and shim tests

`cargo-slot test -j 2 -p galion-deploy`: 35 unit tests (6 new: crash loop cut short with
a non-zero base, stuck `failed`, one `failed` poll is not enough plus counter re-base
plus pending auto-restart, healthy deploy untouched, no NRestarts means the full window,
env parsing; plus `backend::unit_state` parsing), systemd backend and real_build all ok.
The systemd backend scenarios (shimmed `systemctl`, exact call sequences):

```
  OK  failfast-broken: exit 3, 10 calls (gave up on the first poll instead of after 20 tries)
  OK  failfast-no-nrestarts: exit 3, 29 calls (full window)
  OK  failfast-healthy: exit 0, 7 calls
  (the 15 older scenarios unchanged: healthy 5 calls, broken 28, rollback-also-bad 47, …)
```

The `failfast-broken` sequence is `cat, show ExecStart, reset-failed, restart, show
ActiveState/SubState/NRestarts ×2, journalctl, reset-failed, restart, is-active`. With
the flag off, the healthy and broken scenarios still assert the original sequences, so
no extra `show` call appears.

Builds: `cargo fmt --check` and `cargo clippy -p galion-deploy --all-targets -D
warnings` are clean. The x86_64 musl release binary is 586,416 B (+9,272 B) and the
aarch64 musl one 522,208 B (+4,744 B). `cargo check --all-targets` passes for aarch64-linux-android
and armv7-unknown-linux-musleabihf. `scripts/no-python.sh` passes.

## 4. Read-only VPS probe

At 2026-10-21 ~04:08 +02:00 the VPS reported `systemd 255 (255.4-1ubuntu8)`, so
`NRestarts` and `SubState=auto-restart` are both available. `NRestarts=0` on every unit
below, and every unit was `active`.

| unit | Restart | RestartSec | start limit | FAILFAST=2 bound under systemd |
|---|---|---|---|---|
| tuner | always | 2 s | 5 / 10 s | ≈ 2.3 s |
| galion-desk | on-failure | 2 s | 5 / 10 s | ≈ 2.3 s |
| hermes-rs | on-failure | 3 s | 5 / 10 s | ≈ 3.3 s |
| link-watch-rs | always | 3 s | 5 / 10 s | ≈ 3.3 s |
| openmind-memory | always | 5 s | 5 / 10 s | ≈ 5.3 s |
| galion-health | always | 5 s | 5 / 10 s | ≈ 5.3 s |
| galion-one | always | 5 s | 10 / 5 min | ≈ 5.3 s |
| galion-nerve | always | 10 s | 5 / 10 min | ≈ 10.3 s |

With a 10 s start-limit window and RestartSec=2, a broken tuner hits the start limit
(5 starts) after about 8 s. That makes it `failed`, which the two-poll rule also
catches.

## 5. Owner decisions

1. **Turn fail-fast on, and with which N?** It is off by default, so the deploy contract
   is unchanged and runbooks or callers need nothing. The options:
   - (a) keep it off;
   - (b) set `GALION_DEPLOY_FAILFAST=2` in the cutover runbooks' galion-deploy calls
     (recommended: L1 outage ~20 s → ~2-10 s under systemd per the table above, and one
     transient crash of a good build is tolerated);
   - (c) make 2 the built-in default, which changes the documented contract (README and
     docs/cutover say "20 tries");
   - (d) N=1, the fastest option, but the rehearsal shows it rolls back a good build after one
     transient crash.
2. **Not covered by any N:** a build that stays up but never becomes healthy (sim-hang,
   a wrong port, a stuck dependency). It keeps the full window. Shortening that would mean
   tuning `GALION_DEPLOY_HEALTH_TRIES`, which is a separate call because a slow but good
   start would then be rolled back.
3. It still depends on deploy-rust decision 1 (install the Rust galion-deploy on the
   VPS at all). The live bash tool has no fail-fast.

## 6. Open issues

- The systemd timings are derived (no systemd on the box). The `SubState=auto-restart`
  path is proven against the shim and unit tests, not against a real systemd 255. A
  one-off check on a throwaway unit on a systemd host would close this. That must not
  be the production VPS without the owner's go.
- On simsup (no `SubState`) the log shows `state=active/` with an empty sub-state. This
  is cosmetic.
- The rehearsal's `room` test needs commit 5bb27e1 in the clone. That is environmental
  and unrelated to this change.
