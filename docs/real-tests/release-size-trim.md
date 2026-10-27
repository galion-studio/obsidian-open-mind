<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests — slot 294 release-size-trim (2026-10-27, Europe/Zagreb CET +01:00)

Box ports 21640-21649 only. Binaries: static musl release builds of
`swarm/release-size-trim` (base 98466ab9 + `.cargo/config.toml` trim), built with
zig 0.15.2 `cc` wrappers as in `scripts/ucpu-gate.sh` and rust-lld, no CFLAGS override.
aarch64 binaries ran under qemu-aarch64 10.0.13 (Debian `qemu-user`, unpacked into the
slot target dir, not installed system-wide). No deploy, VPS read-only.

## Binaries under test

| Binary | Bytes | sha256 (16) | PT_INTERP | DT_NEEDED | .symtab | size-budget.sh |
|---|---:|---|---:|---:|---:|---|
| x86_64 hermes-gateway | 2,213,824 | c02c5aeb3093bb7e | 0 | 0 | 0 | FAIL (over 2 MiB) |
| x86_64 openmind-memory | 1,743,584 | 3551afa2a2785519 | 0 | 0 | 0 | ok |
| aarch64 hermes-gateway | 1,942,904 | 5a9db869bc351043 | 0 | 0 | 0 | ok |
| aarch64 openmind-memory | 1,588,384 | d07b50d290d02c42 | 0 | 0 | 0 | ok |

## hermes-gateway — `crates/hermes-gateway/tests/real_size_trim.rs` (GALION_REAL=1)

`--mock` provider, fixture key, offline. Start → GET /health → GET /v1/models → 3× POST
/v1/chat/completions with `X-Hermes-Session-Id` → stop → SQLite `rs-state.db` exists →
restart on the same `HERMES_HOME` → session listed in /api/sessions and its messages
returned from SQLite.

x86_64 (port 21646, between 19:20 and 19:30 +01:00):
```
GET /health -> 200 {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}
3x POST /v1/chat/completions (session st294-session-a) -> 200
.../real-size-trim/rs-state.db = 40960 B after stop
after restart: /api/sessions lists st294-session-a; messages hold the marker 6x
test result: ok. 1 passed; 0 failed
```
aarch64 under qemu (port 21647, just before 19:31 +01:00): identical output, `1 passed; 0 failed`.

## openmind-memory — existing suites `real_memory` + `real_schema` (GALION_REAL=1)

`OPENMIND_MEMORY_BIN=<trimmed musl bin> OPENMIND_REAL_PORT_BASE=21640`, `--test-threads=1`
(19:31-19:43 +01:00). Covers every route on a fixture DB, FTS5/BM25 recall, CJK/emoji FTS
round trip, kill -9 mid-write and mid-migration recovery, read-only mode, 10k perf
baseline, import replay, rollback to previous binary.

| Target | real_memory | real_schema |
|---|---|---|
| x86_64 | 7 passed / 0 failed (59.4 s) | 5 passed / 0 failed (45.2 s) |
| aarch64 (qemu) | 7 passed / 0 failed (358.7 s) | 5 passed / 0 failed (238.3 s) |

The two `*_live_8420_*` cases only probe the VPS with `GALION_REAL_VPS=1`, which was not
set; the live comparison was done by hand below.

## Live VPS comparison (read-only, 19:43-19:45 +01:00)

- `systemctl is-active`: hermes-rs, openmind-memory, galion-nerve, gaia-cortex all `active`.
- Live hermes-rs `GET :8642/health` → 200, body byte-identical to the trimmed build's.
- Live openmind-memory `GET :8420/health` → 200; trimmed x86_64 build on box :21648 returns
  the same keys and values (`"fts":true`, `"read_only":false`, stores/services equal)
  except `uptime`.
- Live binaries: `hermes-gateway-b7fbbc78` 2,732,248 B (the oversize build style),
  `openmind-memory-aa77864` 1,702,640 B.
- Nerve `:8670/state` cycle 12, mode live, score 29 grade F; hermes-rs "degrading: 1
  restart(s) in the last hour", 7 pushed errors this cycle (not caused by this slot —
  nothing was deployed). Cortex `:8674/health` ok, mode live, incidents_open 6.

## Gates (host, same `.cargo/config.toml`)

- `cargo fmt --all --check` ok.
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  378 suites, 2004 passed, 0 failed, 18 ignored. hermes-tools is excluded because its
  `sandboxed` tests run openmind-sandbox (sandbox_vps_only); it has no C code.
- `cargo check` armv7-unknown-linux-musleabihf + aarch64-linux-android for hermes-gateway,
  openmind-memory, galion-backup: ok.
- `scripts/no-python.sh` ok.
