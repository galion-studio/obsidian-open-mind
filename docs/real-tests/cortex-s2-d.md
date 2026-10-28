<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: cortex-s2-d (T2.10 event feed, T2.13 hades/sandbox sensor)

- Branch `swarm/cortex-s2-d`, base `swarm/cortex-sensor` e536446. Box only, slot 58 (ports 19580-19581 used).
- Run: 2026-10-20 23:18:52 - 23:19:53 CEST (+02:00). No VPS access (planning phase; the live nerve :8670 is the old
  Oct 18 build without this route, so a live comparison says nothing yet).
- Test: `crates/openmind-nerve/tests/real_cortex_s2_d.rs`, gated `GALION_REAL=1`:

```
CARGO_TARGET_DIR=/workspace/swarm/target/cortex-s2-d GALION_REAL=1 \
  nice -n 10 cargo test --release -j 2 -p openmind-nerve --test real_cortex_s2_d -- --nocapture
```

## What it does

1. The real release `hades-gate` (same workspace, ccaab42 code) with throwaway keys in a temp dir (never printed)
   decides 5 real requests. Its JSONL ledger is the sensor's input.
2. The real release `openmind-nerve serve` on 127.0.0.1:19580 (stub service :19581) with `NERVE_HADES_LEDGER`,
   `NERVE_HADES_AUDIT` (one hermes-tools-format `refused` line, path traversal), `NERVE_HADES_DISABLE_FILE`,
   `NERVE_HADES_WINDOW_S=25`. Probed over real HTTP: `/api/v1/state`, `/api/v1/cortex/events` (JSONL, `since=`,
   `format=json`). Then `hades-gate disable` / `enable`, then the window runs out.

## Output (stderr of the test, verbatim)

```
hades-gate decisions (exit, code): [("admin-safety plan", (2, "admin-safety")), ("forged token", (2, "forged")), ("valid token", (0, "WouldApply")), ("replayed token", (2, "replayed")), ("kill", (2, "action"))]
ledger: 5 lines (events: ["refused", "refused", "consumed", "refused", "refused"])
after 500.969712ms: cycle 1 hades findings ["hades-safety-refusals:warn", "hades-token-abuse:error", "hades-unapprovable-actions:warn", "sandbox-escape-attempts:warn"]
sensor run {"findings":4,"name":"hades","signals":1,"us":51} cycle_ms 22 incidents 1
feed line: {"data":{"cycle":1,"findings":4,"ids":["hades-token-abuse","hades-safety-refusals","hades-unapprovable-actions","sandbox-escape-attempts"],"ids_more":0,"top":{"cause":"hades","component":"hades-gate","evidence_n":3,"id":"hades-token-abuse"}},"kind":"vulnerability","severity":"high","source":"hades","ts":"2026-10-20T23:19:23+02:00"}
since=1: 0 bytes; since=0 format=json: 1 events
disabled: cycle 2 ["hades-off:info", "hades-safety-refusals:warn", "hades-token-abuse:error", "hades-unapprovable-actions:warn", "sandbox-escape-attempts:warn"]
cleared after 30.06133432s: cycle 7 cycle_ms 22 VmRSS:	    3408 kB
test real_cortex_s2_d_feed_and_hades_sensor ... ok
```

Checked by the test: every feed line deserialises into `openmind_schema::NerveEvent`; the feed holds none of the plan
text, target, path, session, token hash, `title`, `fix` or evidence; content type `application/x-ndjson`; `since=<cycle>`
is empty; the info finding `hades-off` is visible in `/api/v1/cortex` but not in the feed; all hades findings and
the hades event clear after the 25 s window. Sensor cost 51 us per cycle, cycle 22 ms, RSS 3.4 MiB (budget: < 50 ms,
< 8 MiB). Processes are killed by the test (`Drop`), temp dir removed.

## Gates (2026-10-20 23:20-23:23 +02:00)

- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok.
- `cargo test --workspace`: 980 passed, 0 failed, 4 ignored (openmind-nerve lib 79, of which 15 new: 11 hades, 4 feed;
  server route test extended).
- `scripts/no-python.sh`: ok. `scripts/spdx-scan.sh`: scanned 363, missing 0.
- `cargo check -p openmind-nerve`: x86_64/aarch64 musl, aarch64-linux-android, armv7-unknown-linux-musleabihf ok.
  Release: x86_64-unknown-linux-musl 1151776 bytes; aarch64-unknown-linux-musl 965016 bytes (`linker=rust-lld`).
