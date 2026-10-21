<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# realscript-port-2: second third of `scripts/real/*.sh` ported to Rust

Swarm wave 4, slot 113, branch `swarm/realscript-port-2`, base
`swarm/integration-2` df42e0f. Written 2026-10-21T02:40:00+02:00
(Europe/Zagreb, CEST +02:00). Rust-only rule (2026-10-20 20:41): no new
`scripts/real/*.sh`; every real test builds/locates release binaries, starts,
probes and tears down from Rust, gated by `GALION_REAL=1`.

## Split

`scripts/real/*.sh` on integration-2 had 26 scripts. Alphabetical (C
locale), thirds of 9 / 9 / 8:

| slot | scripts |
|---|---|
| 112 realscript-port-1 | common, desk-linkwatch, docs-sync, edge-boot, gaia-hades-tokens, gaia-live, gaia-poseidon, hades-approval, hermes-tools |
| **113 realscript-port-2 (this branch)** | **integration-1, lan-discovery, lan-routing, launch-gates, memory-real, nerve-capture-vps, nerve, one-probe, poseidon** |
| 114 realscript-port-3 | real-harness, room, sandbox, survival-heal, tuner-capture, tuner-cutover, ucpu, voice-mouth |

(`tuner-requests.txt` is a data file, not a script. If slots 112/114 split
differently, compare with this table before merging.)

## Port map (all 9 scripts deleted)

| old script | Rust test | notes |
|---|---|---|
| integration-1.sh | `crates/galion-realtest/tests/real_integration_1.rs` (2 tests) | A+B: `galion-realtest stack` plain and `--with-nerve`. C: `gaia heal-tick` -> real `openmind-nerve serve`; the script's `--trust-file/--approval-file` flags no longer exist on integration-2 (gaia-hades-tokens), so the port drives the real `hades-gate keygen`, `gaia heal-plan --out`, `hades-gate mint` CLIs. The old script would fail on integration-2. |
| lan-discovery.sh | `crates/openmind-discovery/tests/real_discovery_cli.rs` | CLI flow; the public-/24 refusal now probes TEST-NET-3 `203.0.113.0/24` instead of `8.8.8.0/24` (repo-check `hosts` rule; same refusal path, no socket either way). |
| lan-routing.sh | `crates/openmind-discovery/tests/real_routing.rs` (rewritten) | Used to `bash` the script; now the full scenario in Rust with typed checks. |
| launch-gates.sh | `crates/repo-check/tests/real_launch_gates.rs` | Still calls `scripts/public-export.sh` (the gate pipeline under test, not a real-test driver). New `LG_ALLOW_MISSING_TOOLS=1`: missing scanners are SKIP and their canary checks are reported as skipped, never as passed. |
| memory-real.sh | `crates/openmind-memory/tests/real_memory.rs` (extended) | New `real_binary_version_and_size`; `Server::drop` now fails the test if a listener is left on its port (the script's final check). |
| nerve-capture-vps.sh | `crates/openmind-nerve/tests/real_vps_capture.rs` | Needs `GALION_REAL=1 GALION_REAL_VPS=1`. Same fixed read-only remote commands; prints file names and sizes only; then `openmind-nerve replay DIR --md` on the capture. |
| nerve.sh | `crates/openmind-nerve/tests/real_nerve_scenario.rs` | Same 6 phases, now with asserts (the script only printed). `NERVE_WARMUP_S` (default 200). |
| one-probe.sh | `crates/galion-one/tests/real_one_static.rs` | Static x86_64 musl binary: readelf NEEDED/INTERP, every route, pairing, lockout, fail-closed, mouth. |
| poseidon.sh | `crates/openmind-queue/tests/real_poseidon.rs` | Steps 1-7 with the same checks plus teardown (51); step 8 is `tests/real_binary.rs`, which now uses cargo's own build of the binary instead of a hard-coded target path. |

Shared plumbing: new module `galion_realtest::realrun` (std + serde_json
+ zagreb-time; in-workspace): `enabled`, `release_bins` (or
`GALION_REAL_BIN_DIR`; the fallback build uses its own
`$CARGO_TARGET_DIR/real-bins` because `cargo test` holds the target-dir
lock), `Proc` (kill-on-drop), `wait_http`, `require_free`, `ports_closed`,
`Scratch` (0700, removed on drop unless `GALION_REAL_KEEP=1`), `Checks`
(named PASS/FAIL lines, asserted at the end so every step is reported),
`port_base` (`GALION_REAL_PORT_BASE` moves a test into another slot range).
Default ports are the original slot ranges; this run used slot 113's
19830-19839 via `GALION_REAL_PORT_BASE=19830` (stack test: 19500-19551,
memory: 19090-19095, fixed as before; all checked free first).

## Run on the box, 2026-10-21 02:15-02:31 (+02:00)

Release binaries built once with `cargo-slot build --release -j 2` and
passed in with `GALION_REAL_BIN_DIR`. Box load average about 1.2 on 8 CPUs.

| test | result | time |
|---|---|---|
| real_routing (lan-routing) | **33/33 PASS** | 02:16:01, 4.3 s |
| real_discovery_cli (lan-discovery) | **18/18 PASS** (rerun after the TEST-NET change: 18/18) | 02:15:57 and 02:30:31, 20.5 s |
| real_integration_1::heal | **11/11 PASS**: `1-healthy exit=0 healthy`, `2-no-token exit=4 refused`, `3-approved exit=0 restarted`; nerve saw gaia.heal.applied, gaia.heal.refused, service.probe; JSONL lines = events; token not in tick output or nerve log | 02:17:43 |
| real_integration_1::stack | **7/7 PASS**: plain `Checks: 77 total, 76 pass, 0 FAIL, 0 known-fail, 1 skipped`; `--with-nerve` `Checks: 81 total, 81 pass, 0 FAIL, 0 known-fail, 0 skipped`; all 52 ports free after each | 02:17:55, 02:18:06 |
| real_launch_gates | **13/13 PASS**, 2 SKIP (see below) | 02:19:32, 23.4 s |
| real_nerve_scenario (nerve) | **24/24 PASS** | 02:20:39-02:26:05, 326 s |
| real_memory (memory-real) | **8 passed** (incl. new version test), 0 listeners left on 19090-19099 | 02:25:36, 40 s |
| real_memory live 8420 (`GALION_REAL_VPS=1`, GET only) | ok: /health 200, 9 paths 401, health shape 12 keys identical to the box binary, /health 20x p50 0.375 ms | 02:27 |
| real_vps_capture (nerve-capture-vps, read-only) | **6/6 PASS** (N=2, gap 10 s) | 02:25:39-02:26:17 |
| real_poseidon (poseidon) | **51/51 PASS** | 02:26:46, 7.6 s |
| real_binary (poseidon step 8) | 1 passed | 02:26:46 |
| real_one_static (one-probe) | **51/51 PASS** | 02:26:52, 6.2 s |

Selected outputs:

```
[02:24:44] flood took 179 ms          (6000 GETs, 8 threads; the curl loop took 4.1 s)
[02:24:50] nerve-api: 100.5 req/s over the last 60 s vs baseline 0.50 req/s (threshold 10.0, x201);
           single source 127.0.0.1 (6033 of 6033 requests); 0% 4xx/5xx
[02:26:05] PASS nerve-api back to normal / PASS flood incident resolved
static galion-one (x86_64 musl): NEEDED 0, no interpreter, 832,768 bytes, sha256 6f19b6ec6395c7ae...
openmind-queue 0.1.0: 752,544 bytes
replay of the live capture: result FAIL, score 66/100 (grade D), 2 error(s), 4 warning(s); system degraded
```

The live-capture replay verdict (FAIL, 66/D) is the live VPS's state as seen
by this build, not a test failure; the test checks that capture and replay
work (exit 0 or 1, never 2).

### launch-gates on this box

`gitleaks` and `cargo-cyclonedx` are not installed on the box any more (only
`cargo-deny` and `comply` in the swarm tools dir), so gate 6 (gitleaks) and
gate 9 (SBOM) were SKIP on HEAD and the gate-6 canary could not be checked.
HEAD: 0 FAIL, 1 WARN (gate 10: 22 public IPv4 literals, 1 quick-tunnel
hostname; owner decides, as before). Canary: gates 2, 3, 5, 7, 8 FAIL as
required, `*.redacted` stayed out, exit 1. The exported tree passed
`cargo check --workspace --locked --offline`.

### VPS contact

Read-only only: `ssh slavik-vps` running `curl -s` GETs on loopback (8670
state/self-eval, 8660 status.json, 8002 health, the 10 health probes), `ss -ltn`
and a 35-minute `journalctl` read, plus the memory test's GETs to 8420.
Nothing was written, restarted or reconfigured there. The capture stayed in a
0700 scratch dir on the box and was deleted by the test; no capture content is
committed.

## Static gates (this branch)

| check | result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok |
| `cargo test --workspace --locked` (GALION_REAL unset) | 220 suites, 1011 passed, 0 failed, 4 ignored |
| `scripts/no-python.sh` | ok |
| `repo-check --copyright "3000 Galion / galion-studio"` | 0 errors, 29 warnings -> PASS |

## How to run

```
export GALION_REAL=1
cargo test --release -p openmind-discovery --test real_routing --test real_discovery_cli -- --test-threads=1 --nocapture
cargo test --release -p galion-realtest --test real_integration_1 -- --test-threads=1 --nocapture
GALION_TOOLS_BIN=/path/to/tools cargo test --release -p repo-check --test real_launch_gates -- --nocapture
cargo test --release -p openmind-memory --test real_memory -- --test-threads=1 --nocapture
cargo test --release -p openmind-nerve --test real_nerve_scenario -- --nocapture
GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_vps_capture -- --nocapture
cargo build --release -p galion-one --target x86_64-unknown-linux-musl
cargo test --release -p galion-one --test real_one_static -- --nocapture
cargo test --release -p openmind-queue --test real_poseidon --test real_binary -- --test-threads=1 --nocapture
```

Optional: `GALION_REAL_BIN_DIR=$CARGO_TARGET_DIR/release` (prebuilt),
`GALION_REAL_PORT_BASE=N` (move ports), `GALION_REAL_KEEP=1` (keep scratch).
