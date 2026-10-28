<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve dep growth: where the +471 KB went, and a lean build (slot 385, F-376-1)

- Branch `swarm/nerve-dep-growth`, base `swarm/nerve-xt-pin-refresh` 52aedbc7.
- Times Europe/Zagreb, CET +01:00. Slot ports 22550-22559 (used 22550-22558).
- No new dependency, Cargo.lock unchanged, nothing deployed, VPS read-only.

## Question

F-376-1: the tree nerve (x86_64 musl, release, `-p openmind-nerve`) is
1 930 048 B, +471 288 B (+32 %) over the live 9d1a4f3 binary (1 458 760 B,
rust-lld; 1 463 072 B with the committed default linker), and 92 % of the
2 MiB ceiling (headroom 167 104 B). Which crate / sensor pays for it, and
what can be cut without losing anything the live nerve uses?

## Method (std tools only, no cargo-bloat)

1. Rebuilt both trees on the box (12:56-12:57 +01:00): base 52aedbc7 =
   1 930 048 B sha ccc551fb... (== slot 376), `git archive 9d1a4f3` with
   rust-lld = 1 458 760 B (== VPS d0c70b87...). Same builds with
   `CARGO_PROFILE_RELEASE_STRIP=false` for symbols.
2. `size -A` per section and `nm -C --size-sort --print-size` grouped by
   crate / nerve module (awk), live vs tree.
3. A/B builds (`-p openmind-nerve` solo, x86_64 musl release `--locked`):
   one feature or sensor removed at a time. Sizes are page-rounded
   (4 096 B steps), so these are the real on-disk savings.

## Where the growth is

Sections (unstripped, live -> tree): `.text` 1 113 150 -> 1 398 076
(+284 926), `.rodata` 102 682 -> 221 356 (+118 674, of which 97 216 is the
embedded RustSec snapshot), `.eh_frame` + hdr +51 192, `.rela.dyn` +4 056,
`.data.rel.ro` +3 904.

Measured cost of each piece (A/B builds, bytes off the 1 930 048 B binary):

| piece | pulls | cost | live config uses it? |
|---|---|---|---|
| `rustsec` sensor total (T2.4) | semver + 97 KB advisory snapshot | 135 168 | no (`NERVE_RUSTSEC_LOCK` unset) |
| - of which embedded snapshot | data/rustsec/advisories.tsv | 98 304 | - |
| - of which sensor code + semver | semver 1.0.28 | 36 864 | - |
| `ingress-lint` sensor (T2.6) | tunnel-lint | 32 768 | no (`NERVE_INGRESS_LINT_CONFIG` unset) |
| `drift` sensor (T2.5) | galion-common (sha256, hex) | 12 288 | no (`NERVE_DRIFT_MANIFEST` unset) |
| all three, `--no-default-features` | | **184 320** | |
| 7 other S2 sensors (no new crate) | std only | 184 320 together | no (none of their env set) |
| - proc_owner | | 45 056 | |
| - surface | musl DNS resolver (`to_socket_addrs`: getaddrinfo, res_msend) | 45 056 | |
| - event_feed | | 24 576 | |
| - connrate | | 20 480 | |
| - hades | | 16 384 | |
| - sshd | | 8 192 | |
| - edgerate | | 4 096 | on by default (no env needed) |
| serde_json `preserve_order` (indexmap, foldhash, equivalent) | | 12 288 | workspace-wide pin (serde-order-pin), kept |
| rest: nerve core growth (incident, reflex_audit, engine, server, replay, score, ...) | | ~86 000 | yes |
| linker (default vs rust-lld) | | 4 312 | - |

(The 7 sensors were measured one at a time from the lean build and as a
group; singles add to 163 840 B, the group saves 184 320 B because they share
code. `preserve_order` and the group were measured in a scratch copy only;
not committed.)

Symbol view (unstripped, top deltas live -> tree, bytes): generic std/core
code instantiated for nerve types +30 178 / +18 644 / alloc +12 625,
`sensors::ingress_lint` +15 952, `sensors::event_feed` +15 535,
`sensors::proc_owner` +15 363, `incident` +13 811, `sensors::rustsec`
+12 706, `reflex_audit` +12 574, `Sensors::from_env` (inlined constructors)
+11 272, `sensors::hades` +10 126, `sensors::connrate` +9 984; crates:
semver +5 078, tunnel_lint +4 947, indexmap +3 660, galion_common (sha256)
inlined into `sensors::drift` +7 006. `evt::recover` -9 126 and
`evt::runtime` -9 807 (removed / slimmed since 9d1a4f3).

**Answer:** the three new crates themselves are small (~15 KB of code); the
growth is the new S2 sensor code (~370 KB incl. the rustsec data) plus ~86 KB
of nerve core work. None of the 10 S2 sensors is switched on by the live
unit's environment (read-only VPS probe below), so on today's config about
368 KB of the binary never runs.

## What is staged (this branch)

`crates/openmind-nerve/Cargo.toml` features, **all default on**:

- `sensor-rustsec` (optional `semver`), `rustsec-snapshot` (the embedded
  97 KB DB; implies `sensor-rustsec`), `sensor-drift` (optional
  `galion-common`), `sensor-ingress-lint` (optional `tunnel-lint`).
- A build without a feature, when the sensor's switch-on variable is set
  anyway, prints `<4>openmind-nerve: NERVE_RUSTSEC_LOCK is set but this build
  has no `sensor-rustsec` feature; that sensor stays off` to stderr (journal)
  instead of dropping the sensor silently.
- `rustsec-snapshot` off + sensor on: the sensor needs `NERVE_RUSTSEC_DB`;
  without it, it reports `rustsec-unreadable` (info) naming
  `NERVE_RUSTSEC_DB`, never a silent clean bill.
- Tests that need a feature carry `required-features` (skipped, not failed,
  in a lean build); new unit tests `optional_sensor_features_follow_the_build`
  and `no_embedded_snapshot_needs_a_db_file` (the latter only in a
  no-snapshot build).

Default build: same features and code, same size 1 930 048 B, same set of
strings, `.text` 48 B smaller; **not** bit-identical to 52aedbc7 (sha
ce1196cd... vs ccc551fb...: the feature set is part of the crate metadata
hash, so LTO lays code out in another order). Any later pin must use the new
sha.

The other 7 S2 sensors were not gated: they pull no crate, and gating them
is a bigger cfg change across tests; measured above so the owner can decide.

## Sizes (x86_64 musl release, solo `-p openmind-nerve`, `--locked`)

| build | x86_64 | headroom to 2 MiB | vs warn 1 887 435 | aarch64 |
|---|---|---|---|---|
| live 9d1a4f3 (rust-lld) | 1 458 760 | 638 392 | under | - |
| base 52aedbc7 | 1 930 048 | 167 104 | WARN | 1 591 776 |
| default (this branch) | 1 930 048 | 167 104 | WARN | 1 591 744 |
| `--no-default-features --features sensor-rustsec,sensor-drift,sensor-ingress-lint` (no snapshot) | 1 831 744 | 265 408 | under | - |
| no ingress-lint | 1 897 280 | 199 872 | WARN | - |
| no drift | 1 917 760 | 179 392 | WARN | - |
| no rustsec | 1 794 880 | 302 272 | under | - |
| `--no-default-features` (lean) | 1 745 728 | 351 424 | under | 1 429 872 |

aarch64 lean smoked under qemu-aarch64 on :22558 (13:19:13 +01:00): `/health`
200 cycle 1 system healthy, `cortex_sensors` = listeners, scanners, edge,
components, ddos, edge-rate; stderr has the compiled-out line for
`NERVE_RUSTSEC_LOCK`. `cargo check --no-default-features` for
aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## Real runs (GALION_REAL=1)

| time (+01:00) | test | result | key lines |
|---|---|---|---|
| 13:15:03-13:16:18 | `real_dep_growth` (new), ports 22550-22552 | 1/1 pass, 68.6 s | default 1 930 048 B :22550 sensors 5 built-ins + rustsec, drift, ingress-lint, edge-rate; vulns rustsec-smallvec-1.6.0, rustsec-time-0.1.45, drift-galion-desk, drift-missing-room. no-snapshot 1 831 744 B :22551 rustsec runs, `rustsec-unreadable` (names NERVE_RUSTSEC_DB), no smallvec finding. lean 1 745 728 B :22552 built-ins + edge-rate only, 3 compiled-out warnings once each, vulns []. `default 1930048, no-snapshot -98304, lean -184320` |
| 13:16:24-13:18:10 | `real_cross_target` `NERVE_XT_PIN=budget`, `NERVE_XT_PORT_BASE=22553` | 5/5 pass, 105 s | x86_64 1 930 048 B ce1196cd (WARN above 90 %, headroom 167 104 B); aarch64 1 591 744 B 4e0e1707; solo == recipe ce1196cd; +471 288 B vs pinned VPS 9d1a4f3; smokes :22553/:22554 (qemu)/:22555 first cycle 0.10/0.21/0.10 s, /health wire order equal; android + armv7 check ok |
| 13:26:20-13:27:21 | `real_dep_growth` re-run after the `<4>openmind-nerve:` log prefix change | 1/1 pass, 61.3 s | same sizes; sha default ce1196cd..., no-snapshot 7c38f3d4..., lean a2f5a6ef...; lean warnings once each |

Ports 22550-22559 free after every run (the tests assert it; `ss -ltn`).

## Live (read-only VPS probe, 2026-10-28 12:56 and 13:19:23 +01:00)

- `galion-nerve` active, NRestarts 0, since 2026-10-27 18:15:00 +01:00;
  `/opt/galion/rust/bin/openmind-nerve-9d1a4f3` sha d0c70b87..., 1 458 760 B.
- Unit environment variable **names** (values not read): NERVE_INTERVAL,
  NERVE_DATA_DIR, NERVE_DRY_RUN, NERVE_FAIL_CYCLES, NERVE_REPORT_EVERY,
  NERVE_EVENT_LOG, NERVE_MODE, NERVE_EVT_ACTIONS, NERVE_EVT_ESCALATE,
  NERVE_ROLLBACK, NERVE_COMPONENTS. No S2 sensor variable, no env file.
- nerve `127.0.0.1:8670/health` 200 cycle 87 score 64 system degraded;
  `/state` `cortex_sensors` = listeners, scanners, edge, components, ddos
  (9d1a4f3 predates the S2 sensors). Cortex `127.0.0.1:8674/health` 200.
- Nothing changed on the VPS.

## Findings

- **F-385-1** None of the gated sensors (or the 7 others) is configured in
  the live unit, so a deploy of the tree default carries ~368 KB of code and
  data that will not run there until someone sets their env.
- **F-385-2** The rustsec snapshot alone is 98 KB of binary (and goes stale:
  it is a fixed advisory-db commit). Shipping it as a file
  (`NERVE_RUSTSEC_DB`, already supported) instead of embedding it keeps the
  sensor and gives 265 KB headroom.
- **F-385-3** `surface` pulls the musl DNS resolver via `to_socket_addrs`
  (getaddrinfo / res_msend, ~10 KB of the 45 KB); the targets are usually
  127.0.0.1 URLs. Parsing IP literals first and resolving only real names
  would not shrink the binary (code still linked) unless names are dropped;
  noted only.
- **F-385-4** `edge-rate` is the only S2 sensor on without env (it is in all
  three builds above).
- **F-385-5** The default build is not bit-identical to 52aedbc7 after the
  feature change (same size, same strings); repins must use ce1196cd.

## Owner decisions

- **D-385-1** Deploy recipe for the next nerve: (a) default (all sensors,
  1 930 048 B, WARN), (b) `--no-default-features --features
  sensor-rustsec,sensor-drift,sensor-ingress-lint` + ship the advisory file
  and set `NERVE_RUSTSEC_DB` (1 831 744 B), or (c) lean (1 745 728 B) until
  those sensors are actually switched on. Recommend (b) if the rustsec
  sensor will be switched on at that deploy, else (c); the env warning makes
  a mismatch visible in the journal.
- **D-385-2** Gate the 7 dependency-free S2 sensors too (another 184 KB),
  or leave them in the default build? Not done here.
- **D-385-3** Keep the workspace-wide `preserve_order` pin (12 KB in the
  nerve); recommend keep.

## Gates (box, 2026-10-28 13:18-13:27 +01:00)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets
  --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok;
  `cargo clippy -p openmind-nerve --all-targets -- -D warnings` ok with
  default, `--no-default-features`, and each single feature
  (`sensor-rustsec`, `sensor-drift`, `sensor-ingress-lint`).
- `cargo test --workspace --no-fail-fast` (excl. openmind-sandbox,
  cgroup-detect, hermes-tools; sandbox_vps_only): 379 suites, 2013 passed,
  0 failed, 18 ignored (base 2011 + 2 new).
- `cargo test -p openmind-nerve`: default 42 suites 355/0/3;
  `--no-default-features` 36 suites 325/0/3 (gated suites skipped by
  required-features).
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 753 scanned, 0 missing.
  Cargo.lock unchanged.
