<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: POSEIDON refusal counters across `kill -9`

Wave-4 slot 161 (`swarm/poseidon-refusal-persist`, base integration-3
625eaf0). Box ports 20310-20312. All times Europe/Zagreb (CEST, +02:00).

## What changed

`openmind-queue` keeps `quota_refusals`, `budget_refusals`,
`backpressure_refusals`, `stale_acks` and `charge_clamped` in a new
`refusals.dat` sidecar in the data directory (`src/refusals.rs`): two
512-byte slots, one `crc32 json` line each with a sequence number, written
alternately with one `pwrite` before the HTTP answer is sent, `sync_data`'d
by the ticker within ~1 s. Before this, a `kill -9` reset them to the last
compaction's values (usually 0). No new dependencies (std + the crate's
existing serde/serde_json). See `docs/poseidon.md`, "Limits and known gaps".

## Box: release binary, `kill -9` loop

`GALION_REAL=1 cargo test -p openmind-queue --test real_refusal_persist`
(after `cargo build --release -p openmind-queue`; the Rust test starts the
release binary from `$CARGO_TARGET_DIR/release/openmind-queue` on 127.0.0.1:20310, SIGKILLs it,
restarts it on the same data directory, compares, and kills it on drop).

| Run | Time | Result |
| --- | --- | --- |
| fix, 24 rounds (default) | 2026-10-21 08:14:18-08:14:22 +02:00 | 2/2 pass; 24 kills, 5 compactions, 6 mid-burst kills; final budget 115, backpressure 113, stale 47 419, all exact after every kill |
| fix, `POSEIDON_KILL_ROUNDS=100` | 2026-10-21 08:15:23-08:15:32 +02:00 | 2/2 pass; 100 kills, 25 compactions, 25 mid-burst kills; final budget 480, backpressure 505, stale 238 623; door test 20 kills, quota 10 / budget 10 exact |
| **base 625eaf0 binary** (negative control, same test) | 2026-10-21 08:15:16 +02:00 | 0/2 pass: `kill 1: left (0, 0) right (1, 0)` and `round 1 budget: left 0 right 9`, i.e. every refusal lost on the first `kill -9` |

What each round checks:

- serial phase: 5-24 refusals of three kinds (backpressure 503 at
  `--max-depth 6`, lease-time `budget_hard_stop`, stale ack 404), a
  compaction in ~1 of 4 rounds, then SIGKILL right after the last answer.
  After restart every counter equals the client's own count exactly, the
  sidecar reports `bad_slots 0`, and the job state (1 hour-long lease, 5
  ready jobs) is unchanged;
- burst phase (every 4th round): 4 client threads send stale acks while the
  server is SIGKILLed 150-350 ms in. Restored `stale_acks` must lie in
  [answers the clients received, requests they sent]. Sample (100-round
  run): round 4 seen 8 568 sent 8 577; round 24 seen 13 019 sent 13 026;
  every one of the 25 bursts restored >= seen, so no answered refusal was
  lost;
- door test: one 429 (agent `q` at `max_pending 1`) or one 402 (agent `b`
  cap 5, cost 9) and then an immediate SIGKILL, 20 times; quota and budget
  counters exact after each restart.

Manual curl check, 2026-10-21 08:20:37 +02:00, port 20312: three 429 and one
402, `kill -9`, restart -> `/stats`:

```
"refusals_sidecar":{"bad_slots":0,"file":"refusals.dat","fresh":false,"restored":true,"restored_seq":4,"seq":4,"syncs":0,"write_errors":0,"writes":0}
"budget_refusals":1
"quota_refusals":3
```

(`syncs 0`: the ticker had not synced before the kill; the counters were
still restored from the page cache, which is exactly the `kill -9` case.)

In-process (`tests/refusals_persist.rs`, port 20311, not gated): restart
without compaction, compaction then more refusals, deleted sidecar (falls
back to the compaction's `Counters`, 4/4), torn newest slot (falls back one
step, 5/4, and the next save repairs it). Unit tests in `refusals.rs`:
max values fit a slot, alternating slots, torn slot, flipped byte (crc),
per-field max restore.

## Gates (box)

- `cargo fmt --all --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: ok
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`
  (08:17:34-08:19:16 +02:00): 1221 passed, 0 failed, 6 ignored, 262 suites
- `openmind-queue` alone: 36 lib (5 new) + 2 bin + 1 real_binary + 2 real_refusal_persist (skip without GALION_REAL) + 1 refusals_persist + 1 server_http, all pass
- x86_64 musl release static-pie 889 632 B; aarch64 musl (rust-lld) static
  758 048 B, `version` runs under qemu-aarch64; aarch64-linux-android and
  armv7-unknown-linux-musleabihf `cargo check` ok
- `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` 0 missing,
  `repo-check` PASS (0 errors, 47 pre-existing warnings)

## VPS (read-only), 2026-10-21 08:15:48-08:16:30 +02:00

- No `openmind-queue` / POSEIDON queue unit, process, binary or data
  directory on the VPS (`systemctl list-units --all`, `pgrep`, `ss -ltn`
  on 19120-19129, `command -v`). The queue is not deployed, so there is no
  live counter to compare; this change has nothing to break there yet.
- Nerve `127.0.0.1:8670/state`: HTTP 200, cycle 6922-6923, mode live,
  healthy 11 / degrading 2 / absent 2; Gaia POSEIDON =
  `["lxc-host:healthy"]` (the nerve maps the host's quotas there, not this
  queue). Cortex `127.0.0.1:8680/api/v1/state`: no answer (not deployed),
  so the nerve is the observation point.
- Disclosure: one probe wrote the nerve JSON to `/tmp/.n161` on the VPS and
  removed it in the same command (no secrets in it); later probes piped
  instead.

## Owner decisions

1. Deploying `openmind-queue` (POSEIDON) on the VPS at all is still open;
   nothing was installed. When it is, the sidecar needs no setup (it is
   created in `--data`).
2. Optional: whether per-agent `budget_blocked` should also persist (needs a
   variable-size record; not done), and whether `budget_refusals` should
   count lease calls instead of lease evaluations during a long-poll
   (behaviour unchanged here, documented).
