<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 390 nerve-evt-mode-port (F-373-2): evt_mode tests off shared fixed ports

Branch `swarm/nerve-evt-mode-port` off `real-ports-batch-2` 7c3de266. Box only; nothing deployed,
no VPS change. Times Europe/Zagreb (CET +01:00), 2026-10-28.

## Problem (F-373-2, slot 373 07:38-07:52)

`crates/openmind-nerve/tests/evt_mode.rs` hard-coded 20760 (e2e nerve), 20761 (openrouter
refusal), 20762 (paid stub), 20763 (paid nerve), 20769 (dead component) for every slot. With
several slots running workspace tests at once:
- `paid_slot_against_stub_respects_caps` failed (stub bind / another slot's nerve on 20763);
- `event_mode_end_to_end` hung 12 min: the blocking `open(fifo, write)` waits for a reader
  forever once its own nerve exited on a bind error;
- the health wait could pass against ANOTHER slot's nerve on the same port;
- `openrouter_endpoint_refused_at_start` expects exit 2, but the nerve binds before the
  refusal check, so a busy 20761 gives exit 1;
- nerve stderr was piped but never drained (could block a chatty nerve).

## Fix (test-only, no binary change)

- Default: ephemeral loopback ports (kernel range 32768-60999 on the box). Stub binds
  `127.0.0.1:0`; refusal test listens on port 0; each nerve gets a kernel-picked free port.
- Readiness = the nerve's own `listening on 127.0.0.1:<port> ` stderr line (printed only
  after its bind succeeded) + `/health` 200 + child still alive. A lost bind race (exit with
  `bind ... Address already in use`) retries on a fresh ephemeral port (max 5).
- `NERVE_EVT_IT_PORT_BASE=<base>` (parsed by `galion_realtest::ports::parse_base`, 10-port
  span): fixed block, e2e +0, refusal +1, stub +2, paid nerve +3, dead component +9; no retry
  (a busy port fails fast with the nerve's bind error). Bad value panics naming the var.
- FIFO writer open on a helper thread, polled every 200 ms: panics with nerve stderr if the
  nerve exited or after 30 s, instead of hanging.
- stderr drained by a thread into a buffer used in failure messages.

No new dependency (galion-realtest was already a dev-dependency).

## Evidence (box)

| time CET | run | result |
|---|---|---|
| 14:50 | `cargo test -p openmind-nerve --test evt_mode` | 3/3 ok, 4.13 s |
| 14:50:32-14:50:37 | 4 concurrent copies of the evt_mode binary (ephemeral) | 4 x 3/3 ok (4.1-5.0 s) |
| 14:50:46 | `NERVE_EVT_IT_PORT_BASE=22600` | 3/3 ok |
| 14:50:50 | foreign `openmind-nerve serve` on 22600 + `NERVE_EVT_IT_PORT_BASE=22600` e2e | fails in 0.15 s: `nerve on :22600 exited at start; stderr: bind 127.0.0.1:22600: Address already in use` (old code: health passed against the foreign nerve / FIFO hang) |
| 14:50:53 | same foreign nerve, default ephemeral | 3/3 ok |
| 14:50:54 | `NERVE_EVT_IT_PORT_BASE=70000` | panics `NERVE_EVT_IT_PORT_BASE: port base "70000" is not a number 1024-65535` |
| 14:51-15:10 + 11th | `env -u GALION_REAL cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` x11 | see below |

Workspace runs (GALION_REAL unset per F-383-10; sandbox/cgroup-detect excluded per sandbox_vps_only):

| run | CET | exit | pass/fail/ignored |
|---|---|---|---|
| 1 | 14:51:53-14:54:32 | 0 | 2033/0/18 |
| 2 | 14:54:32-14:55:51 | 0 | 2033/0/18 |
| 3 | 14:55:51-14:57:10 | 0 | 2033/0/18 |
| 4 | 14:57:10-14:58:29 | 0 | 2033/0/18 |
| 5 | 14:58:29-15:03:36 | 0 | 2033/0/18 |
| 6 | 15:03:36-15:04:59 | 0 | 2033/0/18 |
| 7 | 15:04:59-15:06:20 | 0 | 2033/0/18 |
| 8 | 15:06:20-15:07:41 | 0 | 2033/0/18 |
| 9 | 15:07:41-15:08:57 | 101 | 1971/1/18 |
| 10 | 15:08:57-15:10:20 | 0 | 2033/0/18 |
| 11 | 15:12:42-15:14:03 | 0 | 2033/0/18 |

10 clean runs (1-8, 10, 11): 2033 passed / 0 failed / 18 ignored each. Run 9: F-390-1.

evt_mode was 3/3 ok in every run.

- **F-390-1 (pre-existing flake, not this diff):** run 9 failed `room` lib test
  `default_and_fixture_do_not_open_a_socket` (crates/room/src/lib.rs:112): it compares the
  process's `socket:` fds before/after, but sibling test threads in the same binary open
  sockets meanwhile (left 3 sockets, right 1). Cargo stopped after that binary (1971 tests run).
  Suggest serialising that check or filtering by fds it owns. Not fixed here (out of scope).

## Live nerve (VPS, read-only, 14:52 CET)

`GET 127.0.0.1:8670/health` -> `status ok, system degraded, score 64, cycle 94`;
`/stats` -> event mode with `escalate.mode=delegate`, `paid_enabled=false`, `heartbeat_s=900`,
journal + pidfd on (same contract the e2e test asserts); `galion-nerve.service` active,
1 listener on :8670. Cortex not probed in this slot (test-only change to the nerve suite).
