<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve S2 sensor gates: the other seven sensors behind features (slot 404, D-385-2)

- Branch `swarm/nerve-s2-sensor-gates`, base `swarm/nerve-dep-growth` 8f40f18e.
- Times Europe/Zagreb, CET +01:00. Slot ports 22740-22749 (used 22740-22748).
- No new dependency, Cargo.lock unchanged, nothing deployed, VPS read-only
  except one throwaway run as `nobody` in `/var/tmp` (removed).

## What is staged

`crates/openmind-nerve/Cargo.toml` gets seven more features, **all in
`default`**, none pulls a crate:

| feature | sensor | switch-on (same test as its `from_env`) |
|---|---|---|
| `sensor-sshd` | `sshd` (T2.1) | `NERVE_SSHD=1` |
| `sensor-connrate` | `conn-rate` (T2.7) | `NERVE_CONNRATE_PORTS` set |
| `sensor-proc-owner` | `proc-owner` (T2.8) | `NERVE_PROC_OWNER_USERS` set |
| `sensor-surface` | `surface` (T2.9) | `NERVE_SURFACE_TARGETS` set |
| `sensor-hades` | `hades` (T2.13) | `NERVE_HADES=1` or `NERVE_HADES_LEDGER` / `_AUDIT` / `_DISABLE_FILE` set |
| `sensor-event-feed` | `event-feed` (T2.10) | `NERVE_EVENT_FEED_FILE` set |
| `sensor-edgerate` | `edge-rate` (T2.12) | **on with no env**; off only for `NERVE_EDGE_HOSTS=0/off/false/no` |

- Same scheme as slot 385: `#[cfg(feature)]` on the module and its
  `Sensors::from_env` line; a build without the feature prints one
  `<4>openmind-nerve: <why> but this build has no `<feature>` feature; that
  sensor stays off` line at start-up when the sensor would have run.
- `edge-rate` is the one S2 sensor that runs with no env, so a build without
  `sensor-edgerate` warns also when `NERVE_EDGE_HOSTS` is unset
  (`edge-rate is on by default (NERVE_EDGE_HOSTS unset) ...`). Silence it with
  `NERVE_EDGE_HOSTS=0`. Its engine hooks (`edge_hosts` table, cloudflared
  line feed, `ddos` per-host stream skip, incident wording) are gated too;
  `/state.edge_hosts` stays in the JSON as `null` (same as a sensor turned
  off at run time), so the `/state` shape does not change.
- New public `sensors::optional_switch(env, name)` returns why one of the
  seven would run; it exists in every build, so the lean warning uses the
  same rule the sensor uses. Unit test `optional_switch_mirrors_from_env`
  checks it against each `from_env` on 15 env cases; `s2_sensor_features_follow_the_build`
  checks registration/warning per build.
- Tests that need one of the seven carry `required-features` (skipped, not
  failed, without it): 15 more files (`cortex_replay`, `cortex_s2_c`,
  `cortex_t32_correlate`, the four fuzz harness files, `real_connrate_fix`,
  `real_cortex_merge_all`, `real_cortex_s2_c`, `real_cortex_s2_d`,
  `real_cortex_sensor`, `real_cortex_t32`, `real_edge_rate`, `real_event_feed`)
  and 4 of 385's entries extended (`cortex_s2a_replay`, `real_cortex_s2a`,
  `real_cortex_s2b`, `real_cortex_t20`). One test fn in `no_openrouter.rs`
  (surface) is `#[cfg(feature = "sensor-surface")]`.
- `real_dep_growth` (slot 385): its `no-snapshot` variant now keeps the
  seven in, so it still measures only the advisory snapshot.
- New real test `tests/real_s2_sensor_gates.rs` (GALION_REAL=1).

## Sizes (x86_64 musl release, solo `-p openmind-nerve`, `--locked`, 2026-10-28 17:31-17:34 +01:00)

| build | bytes | headroom to 2 MiB | sha256 (8) |
|---|---|---|---|
| live 9d1a4f3 (VPS, rust-lld) | 1 458 760 | 638 392 | d0c70b87 |
| default (this branch) | 1 930 048 | 167 104 | 865e2ec8 |
| default minus `sensor-sshd` | 1 917 760 | 179 392 | - |
| minus `sensor-connrate` | 1 909 568 | 200 480 | - |
| minus `sensor-proc-owner` | 1 884 992 | 212 160 | - |
| minus `sensor-surface` | 1 880 896 | 216 256 | - |
| minus `sensor-hades` | 1 905 472 | 191 680 | - |
| minus `sensor-event-feed` | 1 897 280 | 199 872 | - |
| minus `sensor-edgerate` | 1 909 568 | 200 480 | - |
| `s2-off`: 385's four features only (seven out) | 1 737 536 | 359 616 | 6a82f475 |
| `--no-default-features --features sensor-edgerate` | 1 569 600 | 527 552 | 3cdb0992 |
| `--no-default-features` (lean, all ten S2 out) | 1 553 216 | 543 936 | 2c35d022 |

Savings vs default: sshd 12 288, conn-rate 20 480, proc-owner 45 056,
surface 49 152, hades 24 576, event-feed 32 768, edge-rate 20 480; singles add
to 204 800 B, the seven together save **192 512 B** (shared code). Slot 385's
lean (seven still in) was 1 745 728 B; the new lean is 376 832 B under the
default and 94 456 B over live 9d1a4f3.

Default is the same size as slot 385's default (1 930 048 B) but **not
bit-identical** (sha 865e2ec8... vs ce1196cd...): the feature set is part of
the crate hash again. Any later pin must use the new sha.

aarch64 musl (rust-lld): default 1 591 728 B, lean 1 276 312 B. Lean smoked
under qemu-aarch64 on :22746 (17:37:10 +01:00): `/health` 200, cycle 1,
`cortex_sensors` = the five built-ins; stderr names `sensor-sshd`,
`sensor-hades` (set) and `sensor-edgerate` (default) once each.
`cargo check` default and `--no-default-features` for aarch64-linux-android
and armv7-unknown-linux-musleabihf ok.

## Real runs (GALION_REAL=1, box)

| time (+01:00) | test | result | key lines |
|---|---|---|---|
| 17:31:08-17:34:33 | `real_s2_sensor_gates` (new) `NERVE_S2G_TABLE=1`, ports 22740-22745 (+22749 closed surface target) | 1/1 pass, 204.8 s | 10 builds (table above). default :22740 cycle 1 runs 5 built-ins + sshd, rustsec, drift, conn-rate, proc-owner, surface, hades, ingress-lint, event-feed, edge-rate; 0 compiled-out lines; `edge_hosts` {"hosts":0,"max_hosts":32,...}. s2-off :22741 runs built-ins + rustsec, drift, ingress-lint; 7 lines, one per feature; `edge_hosts` null. lean :22742 built-ins only, 10 lines, each `<4>openmind-nerve: ` prefixed. lean no env :22743 one line (edge-rate default). lean `NERVE_EDGE_HOSTS=0` :22744 no line. default no env :22745 built-ins + edge-rate, no line. All ports closed after. |
| 17:38:13-17:39:15 | `real_dep_growth` (385, updated) `NERVE_DG_PORT_BASE=22740` | 1/1 pass, 62.2 s | default 1 930 048 B vulns rustsec-smallvec-1.6.0, rustsec-time-0.1.45, drift-galion-desk, drift-missing-room; no-snapshot 1 831 744 B (-98 304, unchanged vs 385) `rustsec-unreadable`; lean 1 553 216 B built-ins only |

## VPS (real server)

Read-only probe 17:31:15 +01:00: `galion-nerve` active, MainPID 184014,
NRestarts 0, since 2026-10-27 18:15:00 +01:00; unit env **names** only:
NERVE_ADDR, NERVE_COMPONENTS, NERVE_DATA_DIR, NERVE_DRY_RUN, NERVE_EVENT_LOG,
NERVE_EVT_ACTIONS, NERVE_EVT_ESCALATE, NERVE_FAIL_CYCLES, NERVE_INTERVAL,
NERVE_MODE, NERVE_REPORT_EVERY, NERVE_ROLLBACK; no env file. No S2 sensor
variable. `127.0.0.1:8670/health` 200, `/state` cycle 105,
`cortex_sensors` = listeners, scanners, edge, components, ddos (9d1a4f3
predates the S2 sensors; no `edge_hosts` key). The cortex is not live; the
nerve is the observer.

Throwaway run 17:37:58-17:38:02 +01:00 in `/var/tmp/s2g-404-20261028173725`
as `nobody` (`env -i`, `NERVE_SYSTEMD=0 NERVE_JOURNAL=0 NERVE_REFLEX=0
NERVE_DRY_RUN=1`), the box-built binaries (sha 865e2ec8 / 2c35d022), all
seven switched on with real VPS inputs: `NERVE_CONNRATE_PORTS=8670=galion-nerve`,
`NERVE_PROC_OWNER_USERS=root,nobody`, `NERVE_SURFACE_TARGETS=http://127.0.0.1:8670/health@200`
(one GET to the live nerve), `NERVE_SSHD=1 NERVE_SSHD_AUTH_LOG=0`,
`NERVE_HADES=1`, an empty event file:

- default :22747 cycle 1, `/health` 200; sensors (findings) listeners 1,
  scanners 0, edge 0, components 0, ddos 0, sshd 0, conn-rate 0,
  proc-owner 2 (`stray-owner`, `proc-owner-unresolved`: real VPS listeners
  owned by a user outside root/nobody, and root process fds `nobody`
  cannot read), surface 0 (live nerve /health answered 200), hades 0,
  event-feed 0, edge-rate 0; `edge_hosts` object; 0 compiled-out lines.
- lean :22748 cycle 1, `/health` 200; built-ins only (listeners 1); `edge_hosts`
  null; 7 compiled-out lines, one per feature (edge-rate: "NERVE_EDGE_HOSTS unset").
- Both killed, ports 22747/22748 free, folder removed; live nerve MainPID
  184014 NRestarts 0 active, `/health` 200 after. Nothing else touched.

## Gates (box, 17:20-17:43 +01:00)

- `cargo fmt --all --check` ok.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox
  --exclude cgroup-detect -- -D warnings` ok.
- `cargo clippy -p openmind-nerve --all-targets -- -D warnings` ok for:
  default, `--no-default-features`, default minus each one of the seven, and
  each one of the seven alone (16 combinations).
- `cargo test -p openmind-nerve`: default 357/0/3 (385: 355 + 2 new);
  `--no-default-features` 211/0/0 (gated suites skipped by required-features).
- `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude
  cgroup-detect` (GALION_REAL unset; sandbox_vps_only): 387 suites,
  2038 passed, 0 failed, 18 ignored.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 0 missing.

## Findings

- **F-404-1** None of the ten S2 sensors is configured in the live unit, and
  live 9d1a4f3 has none of them; a lean build (1 553 216 B) runs exactly the
  live sensor set and is only 94 456 B over live.
- **F-404-2** `edge-rate` is on with no env in the tree default, so deploying
  the default adds a sensor the live nerve does not run today. The lean
  build makes that visible with one journal line.
- **F-404-3** The default build is not bit-identical to 8f40f18e's default
  (same size, sha 865e2ec8 vs ce1196cd); repins must use the new sha.
- **F-404-4** `proc-owner` on the real VPS (as nobody) reports a
  `stray-owner` finding on day one: a listener there is owned by a user
  outside root/nobody. Its users list must be set from the real owner map
  before switching it on, or it will page.
- **F-404-5** Lean test runs skip the gated suites (211 vs 357 tests): CI
  should keep running the default feature set as the main gate and the lean
  set as a build/smoke gate.

## Owner decisions

- **D-404-1** Accept the seven gates, all default on (default build behaves
  as before)? Recommend yes.
- **D-404-2** Recipe for the next nerve deploy (folds into D-385-1):
  (a) default 1 930 048 B (WARN, every sensor available);
  (b) lean + `sensor-edgerate` 1 569 600 B (live set + per-host edge rates);
  (c) lean 1 553 216 B (exactly the live set; set `NERVE_EDGE_HOSTS=0` to
  silence the one line); (d) lean + only the features whose env the unit
  will set at that deploy. Recommend (d), which is (c) while no S2 env is
  planned; any mismatch shows in the journal.
- **D-404-3** For `edge-rate`, warn on unset `NERVE_EDGE_HOSTS` in a build
  without it (staged, as it is on by default) or warn only on an explicit
  value? Recommend staged behaviour.
