<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: gaia-core POSEIDON -> openmind-queue (`swarm/gaia-poseidon`)

Run 2026-10-20 20:24 CEST (+02:00) on the box, slot 24, ports 19240-19249
(19240 = `openmind-queue serve`, 19249 = deliberately unused "unreachable"
address). Release binaries from `CARGO_TARGET_DIR=/workspace/swarm/target/gaia-poseidon`:
`gaia` 724312 bytes, `openmind-queue` 750696 bytes (x86_64 gnu; musl
x86_64 and aarch64 builds also done, see below).

## How to rerun

```
scripts/real/gaia-poseidon.sh                                    # 60 checks, builds, starts, probes, tears down
GALION_REAL=1 cargo test -p gaia --test real_gaia_poseidon       # port 19241, same flow in Rust
```

The script uses the `gaia` CLI for everything Gaia does (status, submit,
inspect, caps, quotas, VRAM, dlq, refused budget writes) and the
`openmind-queue` CLI only for operator actions Gaia must never take (set caps,
quotas, devices; run the test worker that leases and acks, which is what
spends the queue's numbers).

## Result

`RESULT pass=60 fail=0` (script) and `test gaia_poseidon_against_real_queue ... ok`
(gated Rust test). After teardown `ss -ltn` showed nothing on 19240-19249.

Highlights:

* Ownership: `gaia systems` shows the four compute systems owned by POSEIDON
  (`FunctionId::COMPUTE`).
* Queue down: `gaia poseidon status --queue 127.0.0.1:19249` exits 3 and
  POSEIDON is `degraded`; no stand-in answer. A non-loopback queue address is
  a usage error (exit 2).
* Submit / inspect: jobs submitted through gaia come back with `job_id`;
  `gaia poseidon job ID` shows state, cost, priority; an unknown job is
  `live 0`; an unknown job field is rejected by the adapter (exit 5).
* Never spends: global and agent `spent`/`reserved`, `acked`, `leased` were
  byte-identical before and after `status`, `vram`, `caps`, `quotas`, `dlq`,
  `plan-vram 16`, `job` and six refused `budget-caps|quotas|vram-pool run|execute`
  directives (exit 4 each, "read-only through Gaia"). Agent cap unchanged at 100.
* Spend cap: after the operator worker acked two cost-40 jobs (spent 80 of
  cap 100), `gaia poseidon submit --cost 40` -> exit 4, outcome `refused`,
  402: `spend cap hard stop (budget_hard_stop: budget agent-a: spent 80 + cost 40 > cap 100)`;
  cost 20 accepted.
* Quota: agent-b `max_pending 2`: third submit -> exit 4, 429
  `quota exceeded (quota_exceeded: agent agent-b has 2 pending jobs (max_pending 2))`;
  `gaia poseidon quotas` shows `pending:agent-b 2`, `max_pending:agent-b 2`, `quota_refusals 1`.
* Money: `--kind payout` refused by the adapter before any call (exit 4).
* VRAM: 24 GB approved (gpu0), 3 devices, 104 GB planned (lan-4090 planned;
  a priced `rented-h100` sent as approved was kept `planned` by the queue
  because no approved provider row exists); `plan-vram 16` -> dry-run on
  gpu0; `plan-vram 48` fits nothing; a 48 GB job is refused (503
  vram_unavailable) unless `--cpu-fallback`.
* DLQ: a `fail` job with `max_attempts 1` dead-letters and shows in
  `gaia poseidon dlq`.
* Restart: `kill -9` the queue -> gaia exit 3; restart on the same data:
  depth, global/agent budget and caps identical; jobs 3 and 6 still live;
  spent 80 persisted; spend cap and quota still enforced; 3 devices and the
  dead letter persisted; store replayed 19 records, 0 truncated bytes.

## Cross-target builds (same run)

* `cargo build --release --target x86_64-unknown-linux-musl -p gaia -p openmind-queue`: ok (static-pie).
* `cargo build --release --target aarch64-unknown-linux-musl ...` (linker rust-lld): ok (static).
* `cargo check --target aarch64-linux-android` and `armv7-unknown-linux-musleabihf`
  for `gaia`, `gaia-poseidon`, `gaia-core`, `openmind-queue`: ok.

## VPS

No VPS probe was made: no POSEIDON or `openmind-queue` service runs on the
VPS (the VPS registry has no POSEIDON function), so there is nothing to
compare against. Nothing on the VPS was touched.

## Full script output

```
2026-10-20T20:24:32+02:00 == build (release, nice -n 10, -j 3)
    Finished `release` profile [optimized] target(s) in 6.21s
2026-10-20T20:24:38+02:00 gaia 724312 bytes; openmind-queue 750696 bytes; openmind-queue 0.1.0
2026-10-20T20:24:38+02:00 == ownership: the single config point
2026-10-20T20:24:38+02:00 PASS compute systems owned by POSEIDON (FunctionId::COMPUTE) (POSEIDON)
2026-10-20T20:24:38+02:00 == unreachable queue: refused, no stand-in fallback
2026-10-20T20:24:38+02:00   gaia poseidon status --queue 127.0.0.1:19249 -> exit 3: degraded
2026-10-20T20:24:38+02:00 PASS unreachable exit (3)
2026-10-20T20:24:38+02:00 PASS POSEIDON degraded when queue down (degraded)
2026-10-20T20:24:38+02:00   gaia poseidon submit --queue 10.1.2.3:19240 --agent x -> exit 2: 
2026-10-20T20:24:38+02:00 PASS non-loopback queue refused (usage error) (2)
2026-10-20T20:24:38+02:00 == start openmind-queue on 127.0.0.1:19240
2026-10-20T20:24:38+02:00   gaia poseidon status -> exit 0: terraforming
2026-10-20T20:24:38+02:00 PASS status exit (0)
2026-10-20T20:24:38+02:00 PASS POSEIDON terraforming (terraforming)
2026-10-20T20:24:38+02:00 PASS backend (openmind-queue)
2026-10-20T20:24:38+02:00 PASS four systems done (4)
2026-10-20T20:24:38+02:00 == operator sets caps, quotas and VRAM devices (openmind-queue CLI, not Gaia)
2026-10-20T20:24:38+02:00 PASS priced device without approved provider stays planned (planned)
2026-10-20T20:24:38+02:00 == submit and inspect jobs through gaia
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-a --kind echo --cost 40 --priority 5 --job-payload hello -> exit 0: job 1 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:38.656+02:00
2026-10-20T20:24:38+02:00 PASS submit 1 done (0)
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-a --kind echo --cost 40 -> exit 0: job 2 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:38.670+02:00
2026-10-20T20:24:38+02:00 PASS submit 2 done (0)
2026-10-20T20:24:38+02:00   gaia poseidon job 1 -> exit 0: job 1 ready on queue default for agent agent-a (kind echo, attempts 0/5, cost 40, vram 0 GB), created 2026-10-20T20:24:38.656+02:00
2026-10-20T20:24:38+02:00 PASS inspect job 1 live (1)
2026-10-20T20:24:38+02:00 PASS inspect job 1 cost (40)
2026-10-20T20:24:38+02:00 PASS inspect job 1 priority (5)
2026-10-20T20:24:38+02:00   gaia poseidon job 999999 -> exit 0: job 999999 is not live on openmind-queue (acked jobs are not kept, or it never existed)
2026-10-20T20:24:38+02:00 PASS unknown job not live (0)
2026-10-20T20:24:38+02:00   gaia dispatch queues run --payload {"agent":"agent-a","kind":"echo","cost":1,"token":7} --queue 127.0.0.1:19240 -> exit 5: 
2026-10-20T20:24:38+02:00 PASS unknown job field rejected by adapter (5)
2026-10-20T20:24:38+02:00 == Gaia never spends: snapshot budget, run every gaia read command, compare
2026-10-20T20:24:38+02:00   gaia poseidon status -> exit 0: terraforming
2026-10-20T20:24:38+02:00   gaia poseidon vram -> exit 0: 24 GB approved (1 routable devices), 0 GB used, 104 GB planned; goal 1500 GB, gap 1476 GB; devices: gpu0 24 GB approved free 24, lan-4090 24 GB planned (not routable) free 24, rented-h100 80 GB planned (not routable) free 80
2026-10-20T20:24:38+02:00   gaia poseidon caps -> exit 0: global spent 0 reserved 0 cap 1000; caps only, no money moves
2026-10-20T20:24:38+02:00   gaia poseidon quotas -> exit 0: 2 agent(s); default max_pending 100000, max_inflight 1000
2026-10-20T20:24:38+02:00   gaia poseidon dlq -> exit 0: 0 dead-lettered job(s)
2026-10-20T20:24:38+02:00   gaia poseidon plan-vram 16 -> exit 0: 16 GB would go to device gpu0 (no spend)
2026-10-20T20:24:38+02:00   gaia poseidon job 1 -> exit 0: job 1 ready on queue default for agent agent-a (kind echo, attempts 0/5, cost 40, vram 0 GB), created 2026-10-20T20:24:38.656+02:00
2026-10-20T20:24:38+02:00   gaia dispatch budget-caps run --payload 100000 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS budget-caps run refused (read-only) (4)
2026-10-20T20:24:38+02:00   gaia dispatch quotas run --payload 99 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS quotas run refused (read-only) (4)
2026-10-20T20:24:38+02:00   gaia dispatch vram-pool run --payload 80 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS vram-pool run refused (read-only) (4)
2026-10-20T20:24:38+02:00   gaia dispatch budget-caps execute --payload 100000 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS budget-caps execute refused (read-only) (4)
2026-10-20T20:24:38+02:00   gaia dispatch quotas execute --payload 99 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS quotas execute refused (read-only) (4)
2026-10-20T20:24:38+02:00   gaia dispatch vram-pool execute --payload 80 --queue 127.0.0.1:19240 -> exit 4: 
2026-10-20T20:24:38+02:00 PASS vram-pool execute refused (read-only) (4)
2026-10-20T20:24:38+02:00 PASS budget, caps, acks and leases unchanged by gaia ({"g":{"cap":1000,"reserved":0,"spent":0},"a":{"spent":0,"reserved":0},"acked":0,"leased":0})
2026-10-20T20:24:38+02:00 PASS agent-a cap still 100 (100)
2026-10-20T20:24:38+02:00 == operator worker spends 80 units in the queue (lease + ack; Gaia never does this)
2026-10-20T20:24:38+02:00   worker: 2 ack(s)
2026-10-20T20:24:38+02:00   gaia poseidon caps -> exit 0: global spent 80 reserved 0 cap 1000; caps only, no money moves
2026-10-20T20:24:38+02:00 PASS caps: agent-a spent 80 (80)
2026-10-20T20:24:38+02:00 PASS caps: agent-a cap 100 (100)
2026-10-20T20:24:38+02:00 PASS caps: global cap 1000 (1000)
2026-10-20T20:24:38+02:00 == spend-cap refusal surfaced through gaia
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-a --kind echo --cost 40 -> exit 4: openmind-queue refused the job: spend cap hard stop (budget_hard_stop: budget agent-a: spent 80 + cost 40 > cap 100)
2026-10-20T20:24:38+02:00 PASS spend cap -> exit 4 (4)
2026-10-20T20:24:38+02:00 PASS spend cap -> refused (refused)
2026-10-20T20:24:38+02:00 PASS spend cap -> 402 (402)
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-a --kind echo --cost 20 -> exit 0: job 3 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:38.907+02:00
2026-10-20T20:24:38+02:00 PASS cost that fits under the cap accepted (0)
2026-10-20T20:24:38+02:00 == quota refusal surfaced through gaia
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-b --kind echo -> exit 0: job 4 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:38.920+02:00
2026-10-20T20:24:38+02:00 PASS agent-b job 1 (0)
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-b --kind echo -> exit 0: job 5 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:38.930+02:00
2026-10-20T20:24:38+02:00 PASS agent-b job 2 (0)
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-b --kind echo -> exit 4: openmind-queue refused the job: quota exceeded (quota_exceeded: agent agent-b has 2 pending jobs (max_pending 2))
2026-10-20T20:24:38+02:00 PASS quota -> exit 4 (4)
2026-10-20T20:24:38+02:00 PASS quota -> 429 (429)
2026-10-20T20:24:38+02:00   gaia poseidon quotas -> exit 0: 2 agent(s); default max_pending 100000, max_inflight 1000
2026-10-20T20:24:38+02:00 PASS quotas: agent-b pending 2 (2)
2026-10-20T20:24:38+02:00 PASS quotas: agent-b max_pending 2 (2)
2026-10-20T20:24:38+02:00 PASS quotas: refusals counted (1)
2026-10-20T20:24:38+02:00 == money refused (adapter, before the queue)
2026-10-20T20:24:38+02:00   gaia poseidon submit --agent agent-a --kind payout -> exit 4: real-money access is not approved; POSEIDON tracks caps only
2026-10-20T20:24:38+02:00 PASS money kind -> exit 4 (4)
2026-10-20T20:24:38+02:00 == VRAM pool through gaia
2026-10-20T20:24:38+02:00   gaia poseidon vram -> exit 0: 24 GB approved (1 routable devices), 0 GB used, 104 GB planned; goal 1500 GB, gap 1476 GB; devices: gpu0 24 GB approved free 24, lan-4090 24 GB planned (not routable) free 24, rented-h100 80 GB planned (not routable) free 80
2026-10-20T20:24:38+02:00 PASS vram: 24 GB approved (24)
2026-10-20T20:24:38+02:00 PASS vram: 3 devices listed (3)
2026-10-20T20:24:39+02:00 PASS vram: planned GB counts lan + h100 (104)
2026-10-20T20:24:39+02:00   gaia poseidon plan-vram 16 -> exit 0: 16 GB would go to device gpu0 (no spend)
2026-10-20T20:24:39+02:00 PASS plan 16 GB dry-run (dry-run)
2026-10-20T20:24:39+02:00 PASS plan 16 GB -> gpu0 (true)
2026-10-20T20:24:39+02:00   gaia poseidon plan-vram 48 -> exit 0: 48 GB fits no approved device now: offload to CPU or wait; more VRAM needs owner spend approval
2026-10-20T20:24:39+02:00 PASS plan 48 GB fits nothing (true)
2026-10-20T20:24:39+02:00   gaia poseidon submit --agent agent-c --kind echo --vram-gb 48 -> exit 4: openmind-queue refused the job: VRAM unavailable (vram_unavailable: no approved, healthy device can hold 48 GB (largest 24 GB); set cpu_fallback or get a device approved)
2026-10-20T20:24:39+02:00 PASS 48 GB job without cpu fallback -> refused (4)
2026-10-20T20:24:39+02:00 PASS 48 GB job -> 503 (503)
2026-10-20T20:24:39+02:00   gaia poseidon submit --agent agent-c --kind echo --vram-gb 48 --cpu-fallback -> exit 0: job 6 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:39.038+02:00
2026-10-20T20:24:39+02:00 PASS 48 GB job with cpu fallback accepted (0)
2026-10-20T20:24:39+02:00 == dead-letter queue through gaia
2026-10-20T20:24:39+02:00   gaia poseidon submit --agent agent-d --queue-name dl --kind fail --max-attempts 1 -> exit 0: job 7 queued on openmind-queue 127.0.0.1:19240 at 2026-10-20T20:24:39.052+02:00
2026-10-20T20:24:39+02:00   gaia poseidon dlq -> exit 0: 1 dead-lettered job(s): 7
2026-10-20T20:24:39+02:00 PASS dlq lists job 7 (true)
2026-10-20T20:24:39+02:00 == restart persistence: kill -9 the queue
2026-10-20T20:24:39+02:00   gaia poseidon job 3 -> exit 3: openmind-queue unreachable at 127.0.0.1:19240: Connection refused (os error 111); nothing done (no fallback to the stand-in)
2026-10-20T20:24:39+02:00 PASS queue down -> gaia exit 3 (3)
2026-10-20T20:24:39+02:00 PASS depth, budget and caps survive kill -9 ({"depth":{"dead":1,"delayed":0,"leased":0,"ready":4},"g":{"cap":1000,"reserved":0,"spent":80},"a":{"cap":100,"spent":80}})
2026-10-20T20:24:39+02:00   gaia poseidon job 3 -> exit 0: job 3 ready on queue default for agent agent-a (kind echo, attempts 0/5, cost 20, vram 0 GB), created 2026-10-20T20:24:38.907+02:00
2026-10-20T20:24:39+02:00 PASS job 3 still live after restart (1)
2026-10-20T20:24:39+02:00   gaia poseidon job 6 -> exit 0: job 6 ready on queue default for agent agent-c (kind echo, attempts 0/5, cost 0, vram 48 GB), created 2026-10-20T20:24:39.038+02:00
2026-10-20T20:24:39+02:00 PASS job 6 still live after restart (1)
2026-10-20T20:24:39+02:00   gaia poseidon caps -> exit 0: global spent 80 reserved 0 cap 1000; caps only, no money moves
2026-10-20T20:24:39+02:00 PASS spent 80 persisted (80)
2026-10-20T20:24:39+02:00   gaia poseidon submit --agent agent-a --kind echo --cost 40 -> exit 4: openmind-queue refused the job: spend cap hard stop (budget_hard_stop: budget agent-a: spent 80 + cost 40 > cap 100)
2026-10-20T20:24:39+02:00 PASS spend cap still enforced after restart (4)
2026-10-20T20:24:39+02:00   gaia poseidon submit --agent agent-b --kind echo -> exit 4: openmind-queue refused the job: quota exceeded (quota_exceeded: agent agent-b has 2 pending jobs (max_pending 2))
2026-10-20T20:24:39+02:00 PASS quota still enforced after restart (4)
2026-10-20T20:24:39+02:00   gaia poseidon vram -> exit 0: 24 GB approved (1 routable devices), 0 GB used, 104 GB planned; goal 1500 GB, gap 1476 GB; devices: gpu0 24 GB approved free 24, lan-4090 24 GB planned (not routable) free 24, rented-h100 80 GB planned (not routable) free 80
2026-10-20T20:24:39+02:00 PASS VRAM devices persisted (3)
2026-10-20T20:24:39+02:00   gaia poseidon dlq -> exit 0: 1 dead-lettered job(s): 7
2026-10-20T20:24:39+02:00 PASS dead letter persisted (true)
2026-10-20T20:24:39+02:00   gaia poseidon status -> exit 0: terraforming
2026-10-20T20:24:39+02:00 PASS status after restart (0)
2026-10-20T20:24:39+02:00   store: {"compactions":0,"lines":19,"replayed":19,"truncated_bytes":0}
2026-10-20T20:24:39+02:00 == teardown
2026-10-20T20:24:39+02:00 PASS port 19240 closed
2026-10-20T20:24:39+02:00 RESULT pass=60 fail=0
```
