<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: POSEIDON (`openmind-queue`)

> **2026-10-21T02:40:00+02:00:** `scripts/real/poseidon.sh` is gone; its Rust port is `crates/openmind-queue/tests/real_poseidon.rs` (`real_binary.rs` now uses cargo's own build of the binary). New run: [realscript-port-2.md](realscript-port-2.md). The record below is historical.

- Written: 2026-10-20T19:30:51+02:00 (Europe/Zagreb)
- Branch: `swarm/poseidon` (slot 12, ports 19120-19129)
- Command: `scripts/real/poseidon.sh` (needs curl and jq; builds the release binaries with `nice -n 10 ... -j 4` into `/workspace/swarm/target/poseidon`)
- Gated Rust test: `GALION_REAL=1 cargo test -p openmind-queue --test real_binary` (spawns the release binary on 127.0.0.1:19121 and SIGKILLs it)

## What runs

| Port | Process |
| --- | --- |
| 127.0.0.1:19120 | `openmind-queue serve` (release), data in a fresh `mktemp` dir, `--lease-ms 1500 --max-attempts 3 --backoff-base-ms 200 --backoff-max-ms 1000`, `--providers` pointing at a v1 `openmind-provider` registry with one approved LAN row and one pending RunPod row |
| 127.0.0.1:19121 | the gated Rust test's own server (section 8) |
| 127.0.0.1:19122 | `tuner` (release), serving a temp `index.html`: the real box-local endpoint for the probe dry-run |

Checks, all over real HTTP to the real release binary:

1. **Ordering / priority**: priorities 1, 9, 5, 9 lease as 9, 9 (FIFO), 5, 1.
2. **Refusals**: money kind 403, unknown field 400, budget hard stop at lease (cap 25, cost 10: the third lease is refused), over-reported cost clamped to the reservation, cost over cap 402 at the door, `max_pending` 429.
3. **Retry / DLQ** with the built-in worker: `flaky` nacked twice and acked on attempt 3 with 200 ms then 400 ms backoff; `fail` dead after 2 attempts; redrive puts it back to ready; the priority 5 job ran first.
4. **Timeout**: a 700 ms lease left un-acked expires and is redelivered as attempt 2; the old token gets 409; a worker that exits mid-lease (`crash`, exit 86) twice ends in the DLQ via lease expiry.
5. **VRAM pool**: a LAN device stays `planned` until `sync-providers` mirrors the approved provider row; a priced RunPod device sent as `approved` is demoted to `planned` (spend approval = owner); summary 24 GB approved, 80 GB planned, gap 1476 GB to the 1500 GB goal; 48 GB without fallback is 503; 16 GB jobs: the first goes to `lan-gpu-24`, the second waits (`vram_held`) while the `cpu_fallback` job is offloaded to `cpu`; acking frees VRAM for the waiting job. Gaia dispatch: status `done` for all four systems, `plan` is `dry-run`, a payment payload is `refused`.
6. **Crash-restart persistence**: `kill -9` with one job leased, a torn half-line appended to `queue.log`, restart: the torn 31 bytes are cut, depth, counters, budget and VRAM use are identical, the pre-crash lease token still acks, priority order continues (7, 4, 2, 1), ids continue; after `/admin/compact` (75 -> 18 lines) and a normal restart the state is unchanged.
7. **Dry-run probe** (read-only GET, loopback only): tuner `/index.html` 200, the queue's own `/healthz` 200, a `10.0.0.1` URL refused by the worker.
8. The gated Rust real-binary test passes.

No VPS access, no live service touched, no paid call. Every process the script starts is killed by its exit trap; `ss -ltn` showed nothing on 19120-19129 afterwards.

Not done: no GPU, LAN GPU host or llama-server endpoint exists on this box, so the "real local endpoint" for the dry-run is the workspace `tuner` binary plus the queue itself. No read-only VPS probe was made (no POSEIDON-equivalent service runs there; the VPS registry has no POSEIDON function).

## Output of the final run

```text
2026-10-20T19:30:24+02:00 == build (release, nice -n 10, -j 4)
    Finished `release` profile [optimized] target(s) in 0.03s
2026-10-20T19:30:24+02:00 binary /workspace/swarm/target/poseidon/release/openmind-queue 750696 bytes; openmind-queue 0.1.0
2026-10-20T19:30:24+02:00 == start server on 127.0.0.1:19120
2026-10-20T19:30:24+02:00 PASS healthz function (POSEIDON)
2026-10-20T19:30:24+02:00 healthz: {"function":"POSEIDON","ok":true,"service":"openmind-queue","time":"2026-10-20T19:30:24.500+02:00","version":"0.1.0"}
2026-10-20T19:30:24+02:00 PASS Zagreb offset in time
2026-10-20T19:30:24+02:00 == 1. ordering / priority (queue order)
2026-10-20T19:30:24+02:00 PASS lease order prio 9,9,5,1 (FIFO within 9) ( 2 4 3 1)
2026-10-20T19:30:24+02:00 PASS empty queue miss (empty)
2026-10-20T19:30:24+02:00 == 2. refusals: money, budget hard stop, quota, bad input
2026-10-20T19:30:24+02:00 PASS money kind refused (403)
2026-10-20T19:30:24+02:00 PASS unknown field 400 (400)
2026-10-20T19:30:24+02:00 PASS set cap (200)
2026-10-20T19:30:24+02:00 PASS third lease hits budget hard stop (budget_hard_stop)
2026-10-20T19:30:24+02:00 PASS over-report clamped to reservation (10)
2026-10-20T19:30:24+02:00 PASS cost over cap refused 402 (402)
2026-10-20T19:30:24+02:00 PASS capped spent+reserved <= cap (true)
2026-10-20T19:30:24+02:00 PASS max_pending quota 429 (429)
2026-10-20T19:30:24+02:00 == 3. retry with backoff, dead letters, redrive (built-in worker)
2026-10-20T19:30:24.898+02:00 worker=w1 job=12 kind=sleep agent=anon prio=5 attempt=1 device=- result=ack note="slept 50ms" reply=200 {"acked":true,"charged":0,"id":12}
2026-10-20T19:30:24.903+02:00 worker=w1 job=10 kind=flaky agent=anon prio=0 attempt=1 device=- result=nack note="flaky ok_at=3" reply=200 {"charged":0,"id":10,"retry_at":"2026-10-20T19:30:25.100+02:00","state":"delayed"}
2026-10-20T19:30:24.904+02:00 worker=w1 job=11 kind=fail agent=anon prio=0 attempt=1 device=- result=nack note="failed on purpose" reply=200 {"charged":0,"id":11,"retry_at":"2026-10-20T19:30:25.104+02:00","state":"delayed"}
2026-10-20T19:30:25.103+02:00 worker=w1 job=10 kind=flaky agent=anon prio=0 attempt=2 device=- result=nack note="flaky ok_at=3" reply=200 {"charged":0,"id":10,"retry_at":"2026-10-20T19:30:25.503+02:00","state":"delayed"}
2026-10-20T19:30:25.105+02:00 worker=w1 job=11 kind=fail agent=anon prio=0 attempt=2 device=- result=nack note="failed on purpose" reply=200 {"charged":0,"id":11,"retry_at":null,"state":"dead"}
2026-10-20T19:30:25.505+02:00 worker=w1 job=10 kind=flaky agent=anon prio=0 attempt=3 device=- result=ack note="flaky ok_at=3" reply=200 {"acked":true,"charged":0,"id":10}
2026-10-20T19:30:28.508+02:00 worker=w1 idle exit after 6 jobs (last miss "empty")
2026-10-20T19:30:28+02:00 PASS flaky acked on attempt 3 (1)
2026-10-20T19:30:28+02:00 PASS flaky nacked twice first (2)
2026-10-20T19:30:28+02:00 PASS sleep job (prio 5) ran first (job=12)
2026-10-20T19:30:28+02:00 PASS fail job dead after 2 attempts (2)
2026-10-20T19:30:28+02:00 PASS redrive (200)
2026-10-20T19:30:28+02:00 PASS redriven job ready (ready)
2026-10-20T19:30:28+02:00 == 4. lease timeout -> redelivery, stale token fenced
2026-10-20T19:30:29+02:00 PASS expired lease requeued (delayed by 200 ms backoff, then ready) (requeued)
2026-10-20T19:30:29+02:00 PASS last_error says expired (true)
2026-10-20T19:30:29+02:00 PASS redelivered same job (13)
2026-10-20T19:30:29+02:00 PASS attempt 2 (2)
2026-10-20T19:30:29+02:00 PASS old token fenced 409 (409)
2026-10-20T19:30:29+02:00 PASS new token acks (200)
2026-10-20T19:30:29+02:00 PASS worker crashed mid-lease (exit 86) (86)
2026-10-20T19:30:29.854+02:00 worker=crasher job=14 kind=crash agent=anon prio=0 attempt=1 device=- result=crash exiting mid-lease
2026-10-20T19:30:31.051+02:00 worker=crasher2 job=14 kind=crash agent=anon prio=0 attempt=2 device=- result=crash exiting mid-lease
2026-10-20T19:30:32+02:00 PASS crashing job dead after 2 expired leases (dead 2)
2026-10-20T19:30:32+02:00 == 5. VRAM pool: provider-gated devices, GPU->LAN->CPU routing, goal
2026-10-20T19:30:32+02:00 PASS lan device planned until provider sync (planned)
2026-10-20T19:30:32+02:00 PASS sync-providers changed 1 (1)
2026-10-20T19:30:32+02:00 PASS paid device demoted to planned (planned)
2026-10-20T19:30:32+02:00 device note: paid device needs an approved openmind-provider row (owner spend approval); kept as planned
2026-10-20T19:30:32+02:00 PASS approved/planned/gap GB (24/80/1476)
2026-10-20T19:30:32+02:00 PASS 48 GB without fallback 503 (503)
2026-10-20T19:30:32+02:00 PASS first 16 GB job on lan-gpu-24 (15 lan-gpu-24)
2026-10-20T19:30:32+02:00 PASS second skipped (no room), fallback job offloaded to cpu (17 cpu)
2026-10-20T19:30:32+02:00 PASS vram_held miss (vram_held)
2026-10-20T19:30:32+02:00 PASS freed VRAM goes to waiting job (16 lan-gpu-24)
2026-10-20T19:30:32+02:00 PASS gaia vram-pool plan is dry-run (dry-run)
2026-10-20T19:30:32+02:00 PASS gaia money refused (refused)
2026-10-20T19:30:32+02:00 PASS gaia vram-pool status done (done)
2026-10-20T19:30:32+02:00 gaia vram-pool: 24 GB approved (1 routable devices), 16 GB used, 80 GB planned; goal 1500 GB, gap 1476 GB
2026-10-20T19:30:32+02:00 PASS gaia quotas status done (done)
2026-10-20T19:30:32+02:00 gaia quotas: 3 agent(s); default max_pending 100000, max_inflight 1000
2026-10-20T19:30:32+02:00 PASS gaia queues status done (done)
2026-10-20T19:30:32+02:00 gaia queues: 5 ready, 0 delayed, 2 leased, 1 dead (max_depth 100000)
2026-10-20T19:30:32+02:00 PASS gaia budget-caps status done (done)
2026-10-20T19:30:32+02:00 gaia budget-caps: global spent 20 reserved 0 cap none; caps only, no money moves
2026-10-20T19:30:32+02:00 == 6. crash-restart persistence (kill -9, torn tail, replay)
2026-10-20T19:30:32+02:00 killed -9; appended a torn line; log is 65 lines
2026-10-20T19:30:32+02:00 restart: 2026-10-20T19:30:32.371+02:00 openmind-queue POSEIDON listening on 127.0.0.1:19120 data /tmp/poseidon-real.VOlQs3/data replayed 65 truncated_bytes 31 live {"dead":1,"delayed":0,"leased":3,"ready":9}
2026-10-20T19:30:32+02:00 PASS state identical after kill -9 + replay ({"depth":{"dead":1,"delayed":0,"leased":3,"ready":9},"acked":9,"enq":22,"g":{"cap":null,"reserved":0,"spent":20},"vram":16})
2026-10-20T19:30:32+02:00 PASS torn tail cut (true)
2026-10-20T19:30:32+02:00 PASS pre-crash lease acks after restart (200)
2026-10-20T19:30:32+02:00 PASS priority order kept after restart ( 7 4 2 1)
2026-10-20T19:30:32+02:00 PASS ids continue after restart (new id > pre-crash id) (yes)
2026-10-20T19:30:32+02:00 compact: {"lines_after":18,"lines_before":75}
2026-10-20T19:30:32+02:00 PASS state after compaction + restart ({"depth":{"dead":1,"delayed":0,"leased":2,"ready":6},"enq":23,"acked":14})
2026-10-20T19:30:32+02:00 == 7. dry-run probe against a real local endpoint (tuner on 19122), read-only GET
2026-10-20T19:30:32.594+02:00 worker=prober job=24 kind=probe agent=anon prio=0 attempt=1 device=- result=ack note="probe GET http://127.0.0.1:19122/index.html -> 200 (36 bytes)" reply=200 {"acked":true,"charged":0,"id":24}
2026-10-20T19:30:32.595+02:00 worker=prober job=25 kind=probe agent=anon prio=0 attempt=1 device=- result=ack note="probe GET http://127.0.0.1:19120/healthz -> 200 (117 bytes)" reply=200 {"acked":true,"charged":0,"id":25}
2026-10-20T19:30:32.595+02:00 worker=prober job=26 kind=probe agent=anon prio=0 attempt=1 device=- result=nack note="probe refused: loopback http:// URLs only" reply=200 {"charged":0,"id":26,"retry_at":null,"state":"dead"}
2026-10-20T19:30:32.595+02:00 worker=prober max-jobs reached (3)
2026-10-20T19:30:32+02:00 PASS tuner probe 200 (1)
2026-10-20T19:30:32+02:00 PASS self probe 200 (1)
2026-10-20T19:30:32+02:00 PASS non-loopback probe refused (1)
2026-10-20T19:30:32+02:00 == 8. gated Rust real-binary test (SIGKILL + restart on 127.0.0.1:19121)
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s

2026-10-20T19:30:33+02:00 PASS real_binary_survives_sigkill
2026-10-20T19:30:33+02:00 == final stats
{"depth":{"dead":2,"delayed":0,"leased":2,"ready":6},"stats":{"acked":16,"backpressure_refusals":0,"budget_refusals":0,"charge_clamped":0,"dead":3,"delivered":27,"enqueued":26,"expired":4,"nacked":5,"quota_refusals":0,"redelivered":5,"redriven":1,"stale_acks":0,"vram_offloaded":1},"global":{"cap":null,"reserved":0,"spent":20},"vram":{"approved_cents_per_hour":0,"approved_gb":24,"devices":2,"free_gb":8,"gap_gb":1476,"goal_gb":1500,"planned_cents_per_hour":199,"planned_gb":80,"routable_devices":1,"used_gb":16},"store":{"compactions":0,"lines":27,"replayed":18,"truncated_bytes":0},"load":66}
2026-10-20T19:30:33+02:00 server log:
2026-10-20T19:30:24.487+02:00 openmind-queue POSEIDON listening on 127.0.0.1:19120 data /tmp/poseidon-real.VOlQs3/data replayed 0 truncated_bytes 0 live {"dead":0,"delayed":0,"leased":0,"ready":0}
2026-10-20T19:30:32.371+02:00 openmind-queue POSEIDON listening on 127.0.0.1:19120 data /tmp/poseidon-real.VOlQs3/data replayed 65 truncated_bytes 31 live {"dead":1,"delayed":0,"leased":3,"ready":9}
2026-10-20T19:30:32.538+02:00 openmind-queue POSEIDON listening on 127.0.0.1:19120 data /tmp/poseidon-real.VOlQs3/data replayed 18 truncated_bytes 0 live {"dead":1,"delayed":0,"leased":2,"ready":6}
2026-10-20T19:30:33+02:00 RESULT pass=51 fail=0
```
