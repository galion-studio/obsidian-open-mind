<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: Gaia S6.3 LAN fallback (wave-4 slot 214, ports 20840-20849)

Test: `crates/openmind-discovery/tests/real_lan_fallback.rs`, gated with
`GALION_REAL=1`, Rust only (no script, no curl/jq). Base integration-5
0d68f46. Box only: every server binds 127.0.0.1; no real model, no paid
call, no real LAN device; nothing runs on the VPS (VPS = read-only GETs).

## Set-up

| port | process |
|---|---|
| 20840/udp | mDNS port for the test announcer and `find` (not 5353) |
| 20841 | mock_openai `cpu` = CPU fallback (`serve-route --cpu`) |
| 20842 | `openmind-discovery serve-route` (router under test, `--timeout-ms 1000`) |
| 20843 | mock_openai `gpu` = fake GPU provider: registry row `gpu-fake`, kind `gpu`, 80 GB, seeded **pending** |
| 20844 | mock_openai `lan-gpu` = fake LAN provider, announced over mDNS (TXT `vram=24`) |
| 20845 | mock_openai `lan-other`, announced, never approved |
| 20846 | mock_openai `lan-gpu` after it moved |
| 20847 | `openmind-queue serve` (POSEIDON) on the same registry |

mDNS goes to 224.0.0.251:20840 (link-local multicast, never routed) on the
box's own interface; port 20840 is not 5353, so no real responder answers.

## Scenarios and what the run shows (run 2026-10-21 21:06:31-21:06:35 +02:00)

| # | step | result |
|---|---|---|
| find | announce lan-gpu + lan-other, `find` | 2 rows `new`/`pending`; GPU row untouched (pending) |
| S1 | nothing approved | chain `[cpu-local]`, chat -> `cpu-local:ok` |
| S2 | `approve gpu-fake` | chain `[gpu-fake, cpu-local]`, chat -> `gpu-fake:ok` |
| S3 | `approve` LAN (not joined) / `join` LAN | unjoined LAN not in chain; after join chain `[gpu-fake, llamacpp-lan-gpu, cpu-local]`, GPU still first (2/2) |
| S4 | POSEIDON on the same registry | pool = `gpu-fake` 80 + `llamacpp-lan-gpu` 24 only, approved_gb 104, gap_gb 1396 (vs 1500), routable 2; 40 GB and 16 GB jobs lease on `gpu-fake` |
| S5 | GPU hangs (SIGSTOP) | same request: `gpu-fake:io-error,llamacpp-lan-gpu:ok` in 1012 ms (timeout 1000 ms); after SIGCONT `gpu-fake:ok` |
| S6 | GPU killed | `gpu-fake:connect-failed,llamacpp-lan-gpu:ok` in 0 ms; `join gpu-fake` -> down -> chain `[lan, cpu]`, not tried; POSEIDON routable 1; 40 GB job without cpu_fallback -> 503 `vram_unavailable` (largest 24 GB) |
| S7 | LAN killed too | `llamacpp-lan-gpu:connect-failed,cpu-local:ok`; join down -> chain `[cpu-local]`; POSEIDON routable 0; 16 GB job refused without cpu_fallback, accepted with it and leased on `cpu` |
| S8 | lan-gpu back on :20846, announced | `find` -> `status known`, `seen_endpoint :20846`, `moved` logged; registry endpoint still :20844, `pending_endpoint :20846`; join dials old endpoint -> down; chat CPU; moved mock 0 requests |
| S9 | re-approve | endpoint -> :20846, health `unknown`, `moved_from :20844`, `needs: join`; still CPU until join; after join `llamacpp-lan-gpu:ok` (moved mock 1 request); POSEIDON routable 1 |
| S10 | GPU back on :20843, join | chain `[gpu, lan, cpu]` again, chat -> `gpu-fake:ok`; POSEIDON routable 2 |
| S11 | `deny` LAN, then GPU killed | chain `[gpu, cpu]`; chat `gpu-fake:connect-failed,cpu-local:ok` (denied LAN never tried, its mock count unchanged); `join` of the denied row refused (`join-refused` logged); POSEIDON: LAN `rejected`, approved_gb 80, gap 1420; GPU still routable until `join` records it down (finding F2) |
| S12 | CPU killed too | 503 `no_provider`, attempts `cpu-local:connect-failed` (down GPU and denied LAN not tried) |
| S13 | checks | lan-other: 0 requests, still pending, never in the pool; approvals log 17 lines all `+02:00`, no `Z`; router log 17 lines, `+02:00`; ports 20841-20847 free after teardown |

Mock chat counts: cpu 6, gpu 5, gpu (restarted) 1, lan 3, lan (moved) 1, lan-other 0.

Runs (all PASS, ~6.2 s each, release binaries 641592 / 753736 / 327600 bytes
for openmind-discovery / openmind-queue / mock_openai):

| Zagreb time (+02:00) | how | result |
|---|---|---|
| 21:06:31-21:06:35 | `LAN_FALLBACK_BIN_DIR` | PASS (full log above) |
| 21:08:09-21:08:15 | same | PASS, hang fallback 1027 ms |
| 21:08:15-21:08:21 | same | PASS, 1012 ms |
| 21:08:21-21:08:27 | same | PASS, 1013 ms |
| 21:08:38-21:08:59 | test builds release itself into `<target>/real-lan-fallback` | PASS |
| without `GALION_REAL` | | skipped, ok |

Command:

```text
GALION_REAL=1 CARGO_TARGET_DIR=/workspace/swarm/target/gaia-lan-fallback-test \
  cargo-slot test -j 2 --release -p openmind-discovery --test real_lan_fallback -- --nocapture
```

(Two earlier runs at 21:05:49 and 21:06:18 failed on wrong test
expectations, both fixed in the test, not the code: S11 expected POSEIDON
to see the dead GPU without a `join` (it cannot, F2), S12 expected the
down GPU to be tried.)

## Findings (for the owner; nothing changed in code)

- **F1 late duplicate on a hung provider.** In S5 the router timed out on
  the stopped GPU and served the request from LAN, but the GPU mock still
  executed the same request after SIGCONT (its count is 5, not 4). A
  provider that hangs after accepting the request can run it twice. Harmless
  for a chat reply (cost only); not for a non-idempotent call.
- **F2 health is a `join` snapshot.** The router sees a provider die on
  every request but never writes health back (rule 6, read-only router).
  POSEIDON kept the killed GPU `routable` until someone ran `join`
  (S11). Nothing joins on its own.
- **F3 no discovery path creates a GPU row.** `find` only creates `lan`
  rows; `--gpu URL` operator rows live only in the router and are invisible
  to POSEIDON. The fake GPU was seeded by hand as a pending row and then
  admitted with the normal `approve`.
- **F4** `scripts/real/lan-routing.sh` (bash + curl + jq, slot 31, ports
  19310-19316) is still wrapped by `real_routing.rs`; this Rust test covers
  all of its steps plus GPU, hang and all-down. Not removed here.

## VPS (read-only, 2026-10-21 21:06:58-21:07:43 +02:00)

`ssh` reads only: no UDP 5353 / 2084x listener, no TCP 2084x listener,
avahi-daemon inactive, 0 openmind-discovery / mock_openai / serve-route
processes; live POSEIDON 127.0.0.1:19120 `/healthz` 200, `/vram`
approved_gb 0, gap_gb 1500, routable_devices 0 (no LAN device approved
live; S6.4 needs real hardware); nerve 127.0.0.1:8670 `/healthz` 200,
score 61 grade D, mode dry-run; cortex (gaia-cortex c7f56b8, 127.0.0.1:8674)
`/health` ok, mode dry-run, incidents_open 0, escalations_open 0.
Disclosure: the first probe wrote and removed a temp file `/tmp/.glf-n` on
the VPS (curl -o); later probes wrote nothing.
