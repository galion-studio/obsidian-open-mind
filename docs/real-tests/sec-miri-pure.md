<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: sec-miri-pure (slot 190), 2026-10-21 +02:00

## Live VPS captures through Miri (read-only GETs)

15:06 +02:00: `ssh slavik-vps curl -s -i -m 5 http://127.0.0.1:<port><path>`
(GET only, nothing changed on the VPS). Raw responses saved on the box under
`/workspace/swarm/target/sec-miri-pure/capture/` (not committed; checked for
secret-looking words, none).

| Capture | Status | Head | Body | `ts` stamps |
|---|---|---|---|---|
| 8670 /health (nerve) | 200 | 149 B | 149 B | 1 |
| 8670 /state (nerve) | 200 | 151 B | 48226 B | 25 |
| 8670 /events (nerve) | 200 | 149 B | 156 B | 1 |
| 8642 /health (hermes-rs) | 200 | 139 B | 122 B | 0 |
| 8420 /health (memory) | 200 | 91 B | 277 B | 0 |
| 8091 /health (galion-one) | 200 | 108 B | 3 B | 0 |
| 8650 /health | 200 | 323 B | 12 B | 0 |

Under Miri (`-Zmiri-disable-isolation`, 15:07 +02:00):

- `galion-common --test real_miri_capture`: every head parses
  (`parse_response_head`), Content-Length equals the body length, every
  truncation of the head never claims more than its input, all 27 `ts` stamps
  pass `require_rfc3339` (Zagreb +02:00). 1/0, `7 captures, 27 stamps ok`.
- `openmind-schema --test real_miri_capture`: the live `/events` body
  deserializes as `Vec<NerveEvent>` (1 event: galion-deploy nerve-cutover,
  `2026-10-21T09:43:55+02:00`) and round-trips; 6 JSON bodies parse and
  round-trip with serde_json. 1/0.
- Same two tests natively: 1/0 each.

## Nerve observation (cortex not live)

15:05:24 +02:00 `GET 127.0.0.1:8670/state`: cycle 644, mode live, started
09:43:54 +02:00, score 61/D, 11 healthy, 1 degrading, 1 failing-soon, 2 absent
(galion-one-mouth 503 stuck loading 5.4 h). `/health` 200.
`systemctl is-active galion-nerve openmind-memory hermes-rs`: active x3.
No hades-gate service on the VPS, so there is no live hades-gate to compare;
its real-life tests ran on the box.

## Box real-life tests (release, ports 20600-20609)

- 15:07-15:08 `GALION_REAL=1 HADES_REDTEAM_PORT=20601 cargo test --release -p hades-gate --test real_redteam -- --include-ignored`:
  1/0, `REAL TOTAL WouldApply: 0 / 82`, `REAL legit not approved: 0 / 9`.
- 15:08:44-15:09:17 `GALION_REAL=1 GALION_REAL_PORT_BASE=20602 cargo test --release -p galion-common --test real_common -- --include-ignored`:
  2/0 (real_common + real_hammer). tuner 2000/2000 sequential p99 0.38 ms,
  41683 req/s; origin 2000/2000 p99 0.32 ms; malformed 416 -> 400 x411,
  501 x2, 505 x1, closed x2; oversized -> 431/413; slowloris x80 cut off
  (408/503); both alive at the end.
- `real_cli` not run: its port 19021 is hardcoded outside this slot's range.

## Native checks

- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (15:12-15:13): 1217 passed, 0 failed, 6 ignored.
- `cargo clippy --workspace --all-targets (same excludes) -- -D warnings`: ok.
- `cargo fmt --check`, `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` missing 0.
