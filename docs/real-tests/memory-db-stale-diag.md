<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: memory-db-stale-diag (wave-4 slot 388)

Run 2026-10-28 13:30-13:41 CET (+01:00) on the box, slot ports 22580-22589.
Diagnosis and decisions: `docs/ops/memory-db-stale-diag.md`.

## Box, real release binary

`GALION_REAL=1 GALION_MEMORY_SNAPSHOT_DIR=<box copy> cargo test --release -p openmind-memory --test real_freshness -- --test-threads 1`
-> **2/2 pass** (13:34 and again 13:40 CET).

1. `real_freshness_stale_then_fresh_after_capture` (server on 127.0.0.1:22580,
   instance `galion`, legacy root DB with 1 row): before any capture
   `l0_rows=0 l0_last=none`, `stray_root_db=yes`, legacy `l0_last=2026-10-13T11:19:53+02:00`;
   one real `POST /capture` (200) -> `l0_rows=2`, `conversations_last_file=2026-10-28.jsonl`,
   `state=fresh (max_age_h=1)`, exit 0; `default` instance with the root DB mtime set back
   and an empty reader WAL -> `(empty; reader-touched, ignored)`, `state=stale`, exit 3.
   No content / session-key strings in any output (4 needles checked). Port closed after kill.
2. `real_freshness_live_snapshot_copy`: box copy of the live data dir
   (instance DB + WAL and legacy root DB scp'd read-only from the VPS at 13:29 CET, sha256
   prefixes 0ff84fad / f50024d3 / cb0a3a5a; JSONL files recreated **empty** with the VPS mtimes;
   no content committed; copy deleted after the run) -> exit 3, instance
   `l0_rows=2610 l0_last=2026-10-19T14:57:55+02:00`, `l1_rows=216 l1_last=2026-10-18T21:41:44+02:00`,
   legacy root `l0_rows=4 l0_last=2026-10-13T11:19:53+02:00`, `newest_write=2026-10-19T14:57:55+02:00 age_h=215.6`.

Also: x86_64 musl static release binary (1,764,080 B, no PT_INTERP) on the same copy -> exit 3,
same lines; aarch64 musl release builds (1,721,368 B). Negative control: the base 9dda6146 binary
has no `freshness` subcommand (falls to usage, exit 64), so both real tests fail on base.

## Unit tests (`freshness::tests`, 5)

RFC 3339 parse vs known instants (incl. the 2026-10-25 DST hour), empty dir = stale + "missing",
instance DB counted / root DB ignored / no content, default instance reads the root DB,
empty WAL not a write, max-age boundary.

## Gates (13:36-13:41 CET)

`cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox
--exclude cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude openmind-sandbox
--exclude cgroup-detect --exclude hermes-tools` **380 suites 2016/0/18**; openmind-memory after the
last edit 12 suites 58/0/0; `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` ok; repo-check
0 errors / 83 warnings = base 9dda6146 (83). No sandbox tests run on the box.

## VPS read-only probes (2026-10-28 13:27-13:35 CET)

- `systemctl show openmind-memory`: active/running, MainPID 291, NRestarts 0, started
  2026-10-22 23:46:47 +02:00; drop-ins 50-rust-bin (aa77864) + 60-nerve-url.
- `/etc/openmind-memory.env`: names only (HOST, PORT, DATA_DIR, INSTANCE_ID, TDAI_GATEWAY_API_KEY); values not read out.
- `/proc/291/fd`: `instances/galion/vectors.db`, `-wal`, `-shm` (not the root DB).
- GET 127.0.0.1:8420/health 200 `{"status":"ok",...,"read_only":false,"fts":true}`.
- nerve GET :8670/state 200: openmind-memory `healthy` since 2026-10-27T18:15:01+01:00, rss 660 kB.
- cortex GET :8674/health 200.
- `ss`: 0 established connections to :8420 in 15 samples x 2 s.
- `journalctl -u hermes`: Python Hermes stopped 2026-10-18 21:42:17 +02:00; unit masked.
- `/opt/galion/hermes/config.yaml` `memory:` block: `provider: memory_tencentdb`, `memory_enabled: true`
  (read by nothing live; hermes-rs has no memory client).
- `systemctl list-timers`: galion-backup last 13:22:03 CET = legacy root `vectors.db-shm` mtime.

Nothing written, restarted or reconfigured on the VPS; `/opt/galion/openmind` not touched; no deploy.
