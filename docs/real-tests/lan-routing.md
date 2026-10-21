<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: lan-routing (slot 31, ports 19310-19319)

> **2026-10-21T02:40:00+02:00:** `scripts/real/lan-routing.sh` is gone; `crates/openmind-discovery/tests/real_routing.rs` is now the full Rust scenario (it used to shell out to the script). New run: [realscript-port-2.md](realscript-port-2.md). The record below is historical.

- Run: 2026-10-20T20:48:01+02:00 (Europe/Zagreb), on the box, release builds
- Script: `scripts/real/lan-routing.sh` (also `GALION_REAL=1 cargo test -p openmind-discovery --test real_routing`)
- No real models, no paid calls, nothing on the VPS. Mocks: `examples/mock_openai.rs`.
- Ports: 19310/udp mDNS (test announcer, not 5353), 19311 CPU mock, 19312 router,
  19313 LAN GPU mock, 19314 second LAN mock (never approved), 19315 LAN GPU after
  the move, 19316 POSEIDON.
- Result: PASS, 3 consecutive runs (plus 1 via the gated cargo test). Afterwards
  nothing listens on 19310-19319.
- VPS: no probe done. The live services have no LAN-routing endpoint to compare
  (this router is new), and nothing here needs one.

Each `chat ->` line is `X-Galion-Provider | X-Galion-Attempts | reply content`.

## Output (run 3)

```

== 2026-10-20T20:47:21+02:00  build (release, nice, -j 3)
641608 bytes /workspace/swarm/target/lan-routing/release/openmind-discovery
753736 bytes /workspace/swarm/target/lan-routing/release/openmind-queue

== 2026-10-20T20:47:21+02:00  start mock OpenAI servers: cpu :19311, lan-gpu :19313, lan-other :19314

== 2026-10-20T20:47:21+02:00  announce _llamacpp._tcp lan-gpu (24 GB) and lan-other (48 GB) on mDNS port 19310

== 2026-10-20T20:47:21+02:00  find (mDNS browse on :19310)
[{"provider_id":"llamacpp-lan-gpu","status":"new","approval":"pending","endpoint":"http://127.0.0.1:19313"},{"provider_id":"llamacpp-lan-other","status":"new","approval":"pending","endpoint":"http://127.0.0.1:19314"}]
{"id":"llamacpp-lan-gpu","approval":"pending","health":"unknown","endpoint":"http://127.0.0.1:19313","vram_gb":24}
{"id":"llamacpp-lan-other","approval":"pending","health":"unknown","endpoint":"http://127.0.0.1:19314","vram_gb":48}

== 2026-10-20T20:47:23+02:00  start serve-route on 127.0.0.1:19312 (--cpu http://127.0.0.1:19311)
{"chain":[{"endpoint":"http://127.0.0.1:19311","id":"cpu-local","kind":"cpu_local"}],"need":"llm","order":"gpu -> lan -> cpu_local (approved rows only; lan needs join up)"}

== 2026-10-20T20:47:23+02:00  both LAN rows pending: routed to the CPU fallback
chat -> cpu-local|cpu-local:ok|reply from cpu

== 2026-10-20T20:47:23+02:00  approve llamacpp-lan-gpu (not joined yet: still CPU)
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"approve","from":"pending","id":"llamacpp-lan-gpu","note":"real test: approve","to":"approved","ts":"2026-10-20T20:47:23+02:00"}
chat -> cpu-local|cpu-local:ok|reply from cpu

== 2026-10-20T20:47:23+02:00  join llamacpp-lan-gpu (one TCP connect) -> up, routed to the LAN GPU
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"up","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:23+02:00"}
[{"endpoint":"http://127.0.0.1:19313","id":"llamacpp-lan-gpu","kind":"lan"},{"endpoint":"http://127.0.0.1:19311","id":"cpu-local","kind":"cpu_local"}]
chat -> llamacpp-lan-gpu|llamacpp-lan-gpu:ok|reply from lan-gpu
chat -> llamacpp-lan-gpu|llamacpp-lan-gpu:ok|reply from lan-gpu
X-Galion-Provider: llamacpp-lan-gpu
X-Galion-Attempts: llamacpp-lan-gpu:ok

== 2026-10-20T20:47:23+02:00  POSEIDON on :19316 reads the same registry: VRAM pool = approved rows only
{"devices":[{"id":"llamacpp-lan-gpu","kind":"lan","gb":24,"state":"approved","healthy":true,"routable":true}],"approved_gb":24}
{"id":1,"state":null,"error":null}
{"id":1,"device":"llamacpp-lan-gpu","miss":null}

== 2026-10-20T20:47:23+02:00  kill lan-gpu (server and its announcer): the next request falls back to CPU on its own
chat -> cpu-local|llamacpp-lan-gpu:connect-failed,cpu-local:ok|reply from cpu

== 2026-10-20T20:47:23+02:00  join llamacpp-lan-gpu -> down; the router no longer even tries it; POSEIDON marks it not routable
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"down","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:23+02:00"}
chat -> cpu-local|cpu-local:ok|reply from cpu
{"changed":1,"providers":2}
{"devices":[{"id":"llamacpp-lan-gpu","kind":"lan","gb":24,"state":"approved","healthy":false,"routable":false}],"approved_gb":24}
16 GB job without cpu_fallback:
{"detail":"no approved, healthy device can hold 16 GB (largest 0 GB); set cpu_fallback or get a device approved","error":"vram_unavailable"}
16 GB job with cpu_fallback:
{"id":2}
{"id":2,"device":"cpu","miss":null}

== 2026-10-20T20:47:23+02:00  lan-gpu comes back on a NEW port :19315 and is announced there
[{"provider_id":"llamacpp-lan-gpu","status":"known","approval":"approved","endpoint":"http://127.0.0.1:19313","seen_endpoint":"http://127.0.0.1:19315"},{"provider_id":"llamacpp-lan-other","status":"known","approval":"pending","endpoint":"http://127.0.0.1:19314","seen_endpoint":null}]
{"id":"llamacpp-lan-gpu","approval":"approved","health":"down","endpoint":"http://127.0.0.1:19313","pending_endpoint":"http://127.0.0.1:19315"}
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"down","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:25+02:00"}
chat -> cpu-local|cpu-local:ok|reply from cpu

== 2026-10-20T20:47:25+02:00  re-approve llamacpp-lan-gpu: the move is applied, health resets until join
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"approve","from":"approved","health":"unknown","id":"llamacpp-lan-gpu","moved_from":"http://127.0.0.1:19313","needs":"join","note":"real test: moved to :19315","to":"approved","ts":"2026-10-20T20:47:25+02:00"}
chat -> cpu-local|cpu-local:ok|reply from cpu
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"join","health":"up","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:25+02:00"}
chat -> llamacpp-lan-gpu|llamacpp-lan-gpu:ok|reply from lan-gpu
{"changed":1,"providers":2}
{"devices":[{"id":"llamacpp-lan-gpu","kind":"lan","gb":24,"state":"approved","healthy":true,"routable":true}],"approved_gb":24}

== 2026-10-20T20:47:25+02:00  deny llamacpp-lan-gpu: never used again
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"deny","from":"approved","id":"llamacpp-lan-gpu","note":"real test: deny","to":"rejected","ts":"2026-10-20T20:47:25+02:00"}
chat -> cpu-local|cpu-local:ok|reply from cpu
chat -> cpu-local|cpu-local:ok|reply from cpu
llamacpp-lan-gpu is rejected; only an approved device can join (run approve first)
lan-gpu (:19315) chat requests before deny 1, after deny 1
{"changed":1,"providers":2}
{"devices":[{"id":"llamacpp-lan-gpu","kind":"lan","gb":24,"state":"rejected","healthy":true,"routable":false}],"approved_gb":0}

== 2026-10-20T20:47:25+02:00  the pending lan-other was never used (router or POSEIDON)
lan-other chat requests: 0

== 2026-10-20T20:47:25+02:00  router log (stderr)
2026-10-20T20:47:23+02:00 serve-route listening on 127.0.0.1:19312 registry /tmp/lan-routing-real.RHCvW1/providers.json extra [cpu-local=http://127.0.0.1:19311]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> llamacpp-lan-gpu status 200 tried [llamacpp-lan-gpu:ok]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> llamacpp-lan-gpu status 200 tried [llamacpp-lan-gpu:ok]
2026-10-20T20:47:23+02:00 route GET /v1/models -> llamacpp-lan-gpu status 200 tried [llamacpp-lan-gpu:ok]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [llamacpp-lan-gpu:connect-failed, cpu-local:ok]
2026-10-20T20:47:23+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:25+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:25+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:25+02:00 route POST /v1/chat/completions -> llamacpp-lan-gpu status 200 tried [llamacpp-lan-gpu:ok]
2026-10-20T20:47:25+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]
2026-10-20T20:47:25+02:00 route POST /v1/chat/completions -> cpu-local status 200 tried [cpu-local:ok]

== 2026-10-20T20:47:25+02:00  mock request counts
mock-cpu.err: 8 chat
mock-lan-gpu.err: 2 chat
mock-lan-gpu.err.2: 1 chat
mock-lan-other.err: 0 chat

== 2026-10-20T20:47:25+02:00  approvals log
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19313","event":"found","id":"llamacpp-lan-gpu","instance":"lan-gpu._llamacpp._tcp.local.","kind":"llamacpp","origin":"mdns","service_type":"_llamacpp._tcp.local.","ts":"2026-10-20T20:47:23+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19314","event":"found","id":"llamacpp-lan-other","instance":"lan-other._llamacpp._tcp.local.","kind":"llamacpp","origin":"mdns","service_type":"_llamacpp._tcp.local.","ts":"2026-10-20T20:47:23+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"approve","from":"pending","id":"llamacpp-lan-gpu","note":"real test: approve","to":"approved","ts":"2026-10-20T20:47:23+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"up","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:23+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"down","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:23+02:00"}
{"approval":"approved","by":"real-test","endpoint":"http://127.0.0.1:19313","event":"moved","id":"llamacpp-lan-gpu","instance":"lan-gpu._llamacpp._tcp.local.","kind":"llamacpp","needs":"re-approval (approve ID) before the new endpoint is used","origin":"mdns","seen_endpoint":"http://127.0.0.1:19315","service_type":"_llamacpp._tcp.local.","ts":"2026-10-20T20:47:25+02:00"}
{"approval":"approved","by":"real-test","endpoint":"http://127.0.0.1:19313","event":"seen","id":"llamacpp-lan-gpu","instance":"lan-gpu._llamacpp._tcp.local.","kind":"llamacpp","origin":"mdns","seen_endpoint":"http://127.0.0.1:19315","service_type":"_llamacpp._tcp.local.","ts":"2026-10-20T20:47:25+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19314","event":"seen","id":"llamacpp-lan-other","instance":"lan-other._llamacpp._tcp.local.","kind":"llamacpp","origin":"mdns","service_type":"_llamacpp._tcp.local.","ts":"2026-10-20T20:47:25+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19313","event":"join","health":"down","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:25+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"approve","from":"approved","health":"unknown","id":"llamacpp-lan-gpu","moved_from":"http://127.0.0.1:19313","needs":"join","note":"real test: moved to :19315","to":"approved","ts":"2026-10-20T20:47:25+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"join","health":"up","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:25+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19315","event":"deny","from":"approved","id":"llamacpp-lan-gpu","note":"real test: deny","to":"rejected","ts":"2026-10-20T20:47:25+02:00"}
{"approval":"rejected","by":"real-test","endpoint":"http://127.0.0.1:19315","event":"join-refused","id":"llamacpp-lan-gpu","ts":"2026-10-20T20:47:25+02:00"}

== 2026-10-20T20:47:25+02:00  stop router and POSEIDON

== 2026-10-20T20:47:25+02:00  PASS (work dir /tmp/lan-routing-real.RHCvW1)
```
