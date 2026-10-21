<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: lan-discovery (swarm slot 11)

- Run: 2026-10-20T19:24:03+02:00 to 19:24:29+02:00 (Europe/Zagreb), on the box
  (Linux x86_64, `enp0s4` 172.30.0.2/24), branch `swarm/lan-discovery`.
- Command: `scripts/real/lan-discovery.sh` (release build, `nice -n 10`, `-j 4`,
  `CARGO_TARGET_DIR=/workspace/swarm/target/lan-discovery`). Needs `jq`. No Python.
- Ports (slot 11 range only): 19110/udp test mDNS port, 19111 / 19112 / 19113
  tcp stand-in HTTP ports on 127.0.0.1 (`examples/health_stub.rs`), 19114 tcp
  closed on purpose.

## Why port 19110 and not 5353

The test announcer and browse use mDNS port 19110 so no other responder on
the LAN (avahi, a real Exo) answers and nothing on 5353 is disturbed. The
packets are still real multicast to 224.0.0.251 on `enp0s4`, looped back
to the local announcer; the announcer answers the random-port browse with
a legacy-unicast reply (RFC 6762 6.7). The probe touched only 127.0.0.1
(127.0.0.0/24 host .1). Nothing else on 172.30.0.0/24 was probed.

## What was shown

1. Test announcer advertises `_galion._tcp` and `_exo._tcp` (2 startup announcements).
2. `find`: 2 query packets (query + one repeat), 2 responses, both devices stored **pending**.
3. `join` of the pending row is **refused** (exit 1) and logged `join-refused`.
4. `deny galion-gaia-real` → `rejected`; a second `find` keeps it `rejected` (`seen`).
5. `approve galion-gaia-real` → `approved`; `join` → one TCP connect, health `up`.
6. `find --no-mdns --probe 127.0.0.0/24 --probe-hosts 1-1 --rate 10`: 4 attempts in
   300 ms, 3 open; the two announced ports map to the **same** rows (not
   duplicates), the probe-only port 19113 becomes a new **pending** row.
7. `--probe 203.0.113.0/24` (TEST-NET-3) refused before any socket (exit 2).
8. Every approvals-log line has a `+02:00` timestamp; the script fails on any `Z`.
9. All started processes (announcer, 3 stubs) exit or are killed by the script's trap.

## Output (verbatim, work-dir paths are /tmp)

```

== 2026-10-20T19:24:03+02:00  build (release, nice, -j 4)
563632 bytes /workspace/swarm/target/lan-discovery/release/openmind-discovery

== 2026-10-20T19:24:08+02:00  start stand-in HTTP ports on 127.0.0.1:19111, :19112, :19113

== 2026-10-20T19:24:08+02:00  start test announcer: _galion._tcp + _exo._tcp on mDNS port 19110

== 2026-10-20T19:24:09+02:00  find (mDNS browse, 1 query + 1 repeat, 1.5 s)
{"mdns":{"iface":"172.30.0.2","port":19110,"packets_sent":2,"packets_received":2,"responders":["172.30.0.2"]},"devices":[{"provider_id":"exo-gaia-real","status":"new","approval":"pending","endpoint":"http://127.0.0.1:19112","service_type":"_exo._tcp.local."},{"provider_id":"galion-gaia-real","status":"new","approval":"pending","endpoint":"http://127.0.0.1:19111","service_type":"_galion._tcp.local."}]}

== 2026-10-20T19:24:10+02:00  list --approval pending
{"id":"exo-gaia-real","endpoint":"http://127.0.0.1:19112","approval":"pending","health":"unknown","capabilities":["llm"]}
{"id":"galion-gaia-real","endpoint":"http://127.0.0.1:19111","approval":"pending","health":"unknown","capabilities":["llm"]}

== 2026-10-20T19:24:10+02:00  join galion-gaia-real while pending (must be refused)
galion-gaia-real is pending; only an approved device can join (run approve first)

== 2026-10-20T19:24:10+02:00  deny galion-gaia-real
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"deny","from":"pending","id":"galion-gaia-real","note":"real test: deny first","to":"rejected","ts":"2026-10-20T19:24:10+02:00"}

== 2026-10-20T19:24:10+02:00  find again (denied row stays rejected)
[{"provider_id":"exo-gaia-real","status":"known","approval":"pending"},{"provider_id":"galion-gaia-real","status":"known","approval":"rejected"}]

== 2026-10-20T19:24:12+02:00  approve galion-gaia-real
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"approve","from":"rejected","id":"galion-gaia-real","note":"real test: approve after deny","to":"approved","ts":"2026-10-20T19:24:12+02:00"}

== 2026-10-20T19:24:12+02:00  join galion-gaia-real (approved: one TCP connect to 127.0.0.1:19111)
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"join","health":"up","id":"galion-gaia-real","ts":"2026-10-20T19:24:12+02:00"}

== 2026-10-20T19:24:12+02:00  probe 127.0.0.0/24 host .1 only, ports 19111 19112 19113 19114, 10/s
{"probe":{"attempts":4,"elapsed_ms":300,"hosts":"1-1","net":"127.0.0.0/24","open":3,"ports":["galion:19111","exo:19112","ollama:19113","llamacpp:19114"],"rate_per_s":10},"devices":[{"provider_id":"galion-gaia-real","status":"known","approval":"approved","endpoint":"http://127.0.0.1:19111","verified":true},{"provider_id":"exo-gaia-real","status":"known","approval":"pending","endpoint":"http://127.0.0.1:19112","verified":true},{"provider_id":"ollama-127-0-0-1-19113","status":"new","approval":"pending","endpoint":"http://127.0.0.1:19113","verified":true}]}

== 2026-10-20T19:24:12+02:00  probe of a public /24 is refused before any socket
refusing to probe <public /24>: only loopback, RFC 1918 and link-local /24s are allowed

== 2026-10-21T23:50:21+02:00  re-run by slot 225 (sec-testnet-fixtures), release build, no registry written
(the 2026-10-20 run above used a public /24; the fixture is now TEST-NET-3. Same refusal,
same exit code for TEST-NET-1/2/3 and the old public /24, so the test is not weakened.)
refusing to probe 203.0.113.0: only loopback, RFC 1918 and link-local /24s are allowed   (exit 2)
refusing to probe 198.51.100.0: only loopback, RFC 1918 and link-local /24s are allowed  (exit 2)
refusing to probe 192.0.2.0: only loopback, RFC 1918 and link-local /24s are allowed     (exit 2)

== 2026-10-20T19:24:12+02:00  final registry
{"id":"exo-gaia-real","approval":"pending","health":"unknown"}
{"id":"galion-gaia-real","approval":"approved","health":"up"}
{"id":"ollama-127-0-0-1-19113","approval":"pending","health":"up"}

== 2026-10-20T19:24:12+02:00  approvals log (/tmp/lan-discovery-real.2I2hHl/approvals.log)
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19112","event":"found","id":"exo-gaia-real","instance":"gaia-real._exo._tcp.local.","kind":"exo","origin":"mdns","service_type":"_exo._tcp.local.","ts":"2026-10-20T19:24:10+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19111","event":"found","id":"galion-gaia-real","instance":"gaia-real._galion._tcp.local.","kind":"galion","origin":"mdns","service_type":"_galion._tcp.local.","ts":"2026-10-20T19:24:10+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19111","event":"join-refused","id":"galion-gaia-real","ts":"2026-10-20T19:24:10+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"deny","from":"pending","id":"galion-gaia-real","note":"real test: deny first","to":"rejected","ts":"2026-10-20T19:24:10+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19112","event":"seen","id":"exo-gaia-real","instance":"gaia-real._exo._tcp.local.","kind":"exo","origin":"mdns","service_type":"_exo._tcp.local.","ts":"2026-10-20T19:24:12+02:00"}
{"approval":"rejected","by":"real-test","endpoint":"http://127.0.0.1:19111","event":"seen","id":"galion-gaia-real","instance":"gaia-real._galion._tcp.local.","kind":"galion","origin":"mdns","service_type":"_galion._tcp.local.","ts":"2026-10-20T19:24:12+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"approve","from":"rejected","id":"galion-gaia-real","note":"real test: approve after deny","to":"approved","ts":"2026-10-20T19:24:12+02:00"}
{"by":"real-test","endpoint":"http://127.0.0.1:19111","event":"join","health":"up","id":"galion-gaia-real","ts":"2026-10-20T19:24:12+02:00"}
{"approval":"approved","by":"real-test","endpoint":"http://127.0.0.1:19111","event":"seen","id":"galion-gaia-real","instance":"127.0.0.1:19111","kind":"galion","origin":"probe","service_type":"probe:galion","ts":"2026-10-20T19:24:12+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19112","event":"seen","id":"exo-gaia-real","instance":"127.0.0.1:19112","kind":"exo","origin":"probe","service_type":"probe:exo","ts":"2026-10-20T19:24:12+02:00"}
{"approval":"pending","by":"real-test","endpoint":"http://127.0.0.1:19113","event":"found","id":"ollama-127-0-0-1-19113","instance":"127.0.0.1:19113","kind":"ollama","origin":"probe","service_type":"probe:ollama","ts":"2026-10-20T19:24:12+02:00"}

== 2026-10-20T19:24:12+02:00  wait for the announcer to finish its 20 s
{"addr":"127.0.0.1","announcements_sent":2,"duration_s":20,"host":"gaia-real.local.","mdns_port":19110,"queries_seen":4,"replies_sent":4,"services":["gaia-real._galion._tcp.local.","gaia-real._exo._tcp.local."]}
announcer log lines: 8
2026-10-20T19:24:08+02:00 announce-start gaia-real._galion._tcp.local. (galion) port 19111 host gaia-real.local. addr 127.0.0.1 mdns-port 19110
2026-10-20T19:24:08+02:00 announce-start gaia-real._exo._tcp.local. (exo) port 19112 host gaia-real.local. addr 127.0.0.1 mdns-port 19110
2026-10-20T19:24:08+02:00 announce multicast 224.0.0.251:19110
2026-10-20T19:24:09+02:00 query from 172.30.0.2:35369 for _galion._tcp.local.,_exo._tcp.local. -> legacy-unicast reply to 172.30.0.2:35369 (2 answers)

== 2026-10-20T19:24:29+02:00  PASS (work dir /tmp/lan-discovery-real.2I2hHl)
```

## In-process multicast round trip (cargo test)

```
$ GALION_REAL=1 cargo test -p openmind-discovery --test real_lan -- --nocapture
running 1 test
test announcer_is_found_by_browse_over_real_multicast ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.41s
```

Without `GALION_REAL=1` that test prints `skipped` and passes.

## Not covered

- Binding the real 5353 next to a running avahi: no avahi runs on this box
  (`ss -lun` shows nothing on 5353) and announcing on 5353 would put real
  mDNS records on the box LAN, so it was not done. The SO_REUSEADDR /
  SO_REUSEPORT path is the same code, only the port differs.
- No live VPS service runs LAN discovery, so there was no read-only VPS
  comparison to make.
- No real Exo / Ollama / llama.cpp was on the box. They do not announce
  `_exo._tcp` etc. anyway (see docs/lan-discovery.md); they are found by
  `--probe` or through a Galion edge wrapper that runs `announce`.
