<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: vps-version-flags (slot 194, ops S2.2)

All times Europe/Zagreb (CEST, +02:00). Box only; the VPS was only read
(GET nerve `/api/v1/state`, `ss -ltn`, `systemctl show`). Nothing was copied
to, run on or changed on the VPS. Slot ports 20640-20649 (written into env
as addresses for `--check`; only the positive control binds 20640, then it
is killed).

## 1. Rust real test, glibc release build with stamp (15:30:42-15:32:47)

`GALION_REAL=1 cargo test -p galion-common --test real_flags -- --nocapture`
built 34 binaries with `GALION_BUILD_COMMIT=realflags0`,
`GALION_BUILD_TIME=2026-10-21T15:30:00+02:00`, then for each binary ran
`--version`, `--help`, `--check` (passing temp env) and `--check` (empty env),
3 timed runs each plus one `strace -f` run:

* **136 flag cases x 3 runs: every exit code as required, worst wall time 2.4 ms** (limit 100 ms);
* strace: 0 flagged syscalls in all 136 traces (one execve, no `socket`/`bind`/`listen`/`connect`/any network call, no clone/fork, no open for write/create, no mkdir/unlink/rename/chmod);
* `ss -Hltnup` before/after each binary: no new listener owned by it, nothing on 20640-20649;
* every `--version` line equals `<name> 0.1.0 (commit realflags0, built 2026-10-21T15:30:00+02:00)`.
* hermes-gateway with a broken `config.yaml` and `HERMES_RS_PORT=notaport`: `--version` 0 (1.6 ms) and `--help` 0 (1.4 ms) without opening config.yaml (strace); `--check` 1 (`check failed: config error: parse .../config.yaml: bad flow list`); plain start 2 (config error). The startup checks do not run on the early flags.
* Positive controls (harness can see a bind): tuner start on `192.0.2.1:20640` under strace -> `bind(` seen; tuner start on `127.0.0.1:20640` -> ss showed `tcp 127.0.0.1:20640 users:(("tuner",pid=1509964,fd=5))`, then killed.

`test result: ok. 1 passed; 0 failed` (123.8 s, mostly the release build).

`--check` with an empty environment exits 1 where the box lacks the VPS
paths (expected; the report says what is missing): desk-hermes
(/opt/galion/hermes), galion-deploy (/var/backups/galion/deploy),
galion-voice-serve (/var/lib/galion/voice-models), hermes-gateway (no
API_SERVER_KEY), one-defence (guardian.db), openmind-memory, openmind-nerve
(data dirs), tuner (/opt/galion/tuner/www). Every other binary exits 0.

## 2. Same test on static x86_64 musl binaries (15:35:43)

`GALION_REAL=1 GALION_REAL_BIN_DIR=<target>/x86_64-unknown-linux-musl/release ...`:
34 binaries, 136 cases x 3 + strace, **worst 3.4 ms, failures 0**, hermes broken-config
and both controls as above. `test result: ok` (1.18 s).

## 3. aarch64 musl static under qemu-aarch64

34 binaries built static (rust-lld; zig cc wrapper for bundled SQLite, per
shell only). `env -i qemu-aarch64 <bin> --version|--help|--check`: 102 runs,
0 bad exits (`--check` 0 or 1). Example:
`hermes-gateway 0.1.0 (commit 8e854ea, built 2026-10-21T15:36:53+02:00)`.
aarch64-linux-android and armv7-unknown-linux-musleabihf `cargo check`: ok.
`scripts/ucpu-gate.sh` (ZIG set): RESULT ok (grep, build, static, size, check).

## 4. Before / after on the box (15:44:01-15:44:19)

Base 42de672 release vs this branch, same env, `timeout 2`, `ss -Hltnp` at 0.7 s:

| binary + `--version` | base 42de672 | this branch |
|---|---|---|
| galion-health (`GALION_HEALTH_ADDR=127.0.0.1:20641`) | **bound 127.0.0.1:20641 and served** (`listening on 127.0.0.1:20641 checks=12`) until killed | prints version line, exit 0, no listener |
| hermes-gateway (no key) | loaded config, **created state.db**, then `refusing to start: API_SERVER_KEY is missing` | version line, exit 0 |
| hermes-gateway (fake 23-char test key) | **bound 127.0.0.1:20640** (`listening on 127.0.0.1:20640 mock=false`), created state.db | version line, exit 0, HERMES_HOME untouched |
| galion-backup | exit 1, usage error | version line, exit 0 |

This is the F1 case galion-deploy guards with its 5 s bound: on the VPS the
old binaries would hit an in-use port (or worse, a free one).

## 5. Workspace checks

* `cargo fmt --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: **1230 passed, 0 failed, 6 ignored**.
* First run caught `galion-wallstreet` `no_network_client_in_source` (its deps are pinned; galion-common has an HTTP client) -> wallstreet got a local copy of the flags instead; green after.
* `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 507 scanned, 0 missing; `repo-check` 1384 files, 0 errors, PASS.

## 6. VPS read-only observation (nerve; cortex not deployed)

* 15:32:58: nerve `/api/v1/state` 200, 46644 B, cycle 699, mode live, 11 healthy / 1 degrading / 1 failing-soon / 0 failing / 2 absent, score 61 D (pre-existing). Listeners: 22, 19120, 20241, 20242, 25, 8002, 8080, 8091, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, 53. `/opt/galion/rust/bin`: galion-backup, galion-deploy, galion-health, galion-nerve(-shadow), galion-one, hermes-gateway(-aa77864), link-watch, openmind-memory(-aa77864), openmind-nerve-aa77864 + scripts. All 8 checked units active.
* 15:44:29: nerve cycle 722, same counts; 19 listeners; NRestarts=0 for galion-nerve (up since 09:43:54), hermes-rs (09:46:22), openmind-memory (09:45:26), tuner (04:32:32), galion-health (Oct 18 22:14:38).
* The empty-env `--check` defaults match the live ports: galion-one 8091, openmind-memory 8420, hermes-gateway 8642, galion-desk 8646, tuner 8650, galion-health 8660, openmind-nerve 8670, openmind-queue 19120. galion-origin 8655, room 4020 and galion-voice-serve 8003 are not listening on the VPS (not deployed).

Every deployed Rust binary on the VPS is covered by this branch. None of the
new binaries were run on the VPS: the row is box only. Running
`<new> --check` there as an ordinary user, against the real env, is the
next step (owner decision D-194-2).
