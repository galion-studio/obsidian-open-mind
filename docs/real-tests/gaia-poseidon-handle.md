<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: gaia-poseidon-handle (slot 218)

Branch `swarm/gaia-poseidon-handle`, base integration-5 0d68f46. Box only,
nothing deployed, ports 20880-20889 (used 20880-20882). VPS: read-only GETs.

## Box: `GALION_REAL=1 cargo test -p gaia --test real_poseidon_handle -- --nocapture --test-threads 1`

Last run 2026-10-21 22:08 +02:00: **3 passed, 0 failed** (also 3/3 on four
earlier runs 22:00-22:06). Release `openmind-queue` built from this branch.
POSEIDON's `SubordinateFunction::handle` runs in process behind
`Gaia::dispatch` (ownership routing, judge, bus); no directive was contained.

### A: real openmind-queue on 127.0.0.1:20880 (throwaway data dir)

```
A vram-pool status "16" -> done | 24 GB approved (1 routable devices), 0 GB used, 0 GB planned; goal 1500 GB, gap 1476 GB; devices: gpu0 24 GB approved free 24
A vram-pool plan "" -> rejected | payload must be the GB needed, e.g. 40
A vram-pool plan "16" -> dry-run | 16 GB would go to device gpu0 (no spend)
A vram-pool run "" -> refused | vram-pool run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A vram-pool run "16" -> refused | vram-pool run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A vram-pool execute "" -> refused | vram-pool execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A vram-pool execute "16" -> refused | vram-pool execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A quotas status "" -> done | 1 agent(s); default max_pending 100000, max_inflight 1000
A quotas status "16" -> done | 1 agent(s); default max_pending 100000, max_inflight 1000
A quotas plan "" -> dry-run | quotas: no plan needed; use status
A quotas plan "16" -> dry-run | quotas: no plan needed; use status
A quotas run "" -> refused | quotas run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A quotas run "16" -> refused | quotas run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A quotas execute "" -> refused | quotas execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A quotas execute "16" -> refused | quotas execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A queues status "" -> done | 0 ready, 0 delayed, 0 leased, 0 dead (max_depth 100000)
A queues status "16" -> done | job 16 is not live on openmind-queue (acked jobs are not kept, or it never existed)
A queues plan "" -> dry-run | queues: no plan needed; use status
A queues plan "16" -> dry-run | queues: no plan needed; use status
A budget-caps status "" -> done | global spent 0 reserved 0 cap none; caps only, no money moves
A budget-caps status "16" -> done | global spent 0 reserved 0 cap none; caps only, no money moves
A budget-caps plan "" -> dry-run | budget-caps: no plan needed; use status
A budget-caps plan "16" -> dry-run | budget-caps: no plan needed; use status
A budget-caps run "" -> refused | budget-caps run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A budget-caps run "16" -> refused | budget-caps run: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A budget-caps execute "" -> refused | budget-caps execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A budget-caps execute "16" -> refused | budget-caps execute: read-only through Gaia; caps, quotas and VRAM devices are set by the operator on openmind-queue, and Gaia never spends budget
A read-only batch: 28 + 4 directives, money view unchanged: {"acked":0,"agents":{"agent-a":{"budget_blocked":0,"cap":50,"inflight":0,"pending":0,"quota":[100000,1000],"remaining":50,"reserved":0,"spent":0}},"delivered":0,"enqueued":0,"global":{"cap":null,"rese
A submit 1 acked by operator worker; hard stop 402; job 2 live: job 2 ready on queue default for agent agent-a (kind echo, attempts 0/5, cost 10, vram 0 GB), created 2026-10-21T22:08:02.966+02:00
A lease-time budget block: blocked:agent-a=2, spent:agent-a=Some(30)
A kill -9: refused unreachable, POSEIDON degraded, events ["budget-hard-stop", "poseidon-unreachable"]
A restart: job 2 survived, POSEIDON terraforming again; state {"backend":"openmind-queue","addr":"127.0.0.1:20880","reachable":"yes","load":0,"calls":52,"submitted":2,"refusals":15,"last_error":null,"identity":"0.1.0","contract":"ok","violations":0,"budget":"r
```

Every system x {status, plan, run, execute} x {"", "16"} (28) plus 4 more
went through the strict contract parser with 0 violations. The read-only
batch left spend, reservations, leases, deliveries, acks and enqueues
identical. `blocked:agent-a=2` is the queue's lease-time block counter, which
the pre-218 adapter never reported (read it as a bool).

### B: impostor (not openmind-queue) on 127.0.0.1:20881

```
B impostor saw 11 requests, all GET /healthz; POSEIDON degraded; last: contract violation: GET /healthz names service "galion-one", expected "openmind-queue" (another program on this port?) (openmind-queue at 127.0.0.1:20881); nothing done (no fallback to the stand-in)
```

### C: look-alike breaking the contract on 127.0.0.1:20882

```
C look-alike: 5 requests; refused off-contract; POSEIDON degraded; contract violation: dispatch answered for system Some("queues"), asked "vram-pool" (openmind-queue at 127.0.0.1:20882); nothing done (no fallback to the stand-in)
```

All processes were killed by the test (Drop) and the data dir removed;
`ss -ltn` after the run: ports 20880-20889 free, no leftover queue.

## VPS (read-only, `ssh slavik-vps`, GETs and status only)

2026-10-21 22:04:56-22:05:39 +02:00:

* A live `openmind-queue` exists: `openmind-queue.service` active since
  08:27:54 +02:00, `/usr/local/bin/openmind-queue serve --bind
  127.0.0.1:19120 --data /var/lib/openmind-queue`, sha256 8a7f7faaaa864e0a….
* `GET /healthz` -> `{"function":"POSEIDON","ok":true,"service":"openmind-queue",...,"version":"0.1.0"}`:
  passes the new identity handshake (`contract::check_identity`).
* `GET /gaia/status` lists exactly gaia-core's four compute systems.
* `GET /stats`: 2 agents (deploy-check probes), `"budget_blocked":0` (a
  u64), global spent 0, acked 1, budget_refusals 1, quota_refusals 3.
* `GET /vram`: 0 devices.
* Captures committed in `crates/gaia-poseidon/tests/live/`, replayed by
  `tests/live_shapes.rs` (3/3): handshake ok, every status/plan on contract,
  `max_pending:deploy-check=100000`, `blocked:` reported as the count.
* `POST /gaia/dispatch` was **not** called on the VPS (no POSTs, rule 4).
* Nerve `127.0.0.1:8670/state`: HTTP 200, 37874 B (open alert unrelated:
  galion-one mouth 503 "loading"). Cortex `127.0.0.1:8674/health`:
  `ok:true, mode:dry-run, incidents_open:0, escalations_open:0, events_in:23`.
  No POSEIDON/queue component in the nerve's view; Gaia is not deployed, so
  the cortex has nothing of this branch to observe yet.

## Gates (box)

* `cargo fmt --all --check` ok; `cargo clippy --workspace --exclude
  openmind-sandbox --exclude cgroup-detect --all-targets -D warnings` ok
  (only pre-existing vendored candle-core warnings).
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
  --no-fail-fast`: 270 suites, 1269 passed, 0 failed, 6 ignored (22:03).
  One earlier fail-fast run hit `galion-deploy systemd_backend_scenarios`
  (timing under load, phase=install vs verify; crate does not depend on
  gaia or the queue); it passed in the full rerun.
* Final head: gaia, gaia-core, gaia-poseidon, openmind-queue 23 suites,
  135 passed, 0 failed.
* x86_64 + aarch64 musl release builds of `gaia` and `openmind-queue` ok;
  `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok.
* `scripts/no-python.sh`: ok.
