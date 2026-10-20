<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: tunnel-lint (slot 35, ports 19350-19359)

Run: `scripts/real/tunnel-audit.sh` on the box, 2026-10-20 20:45:59 +02:00, exit 0, `PASS`.
Also `GALION_REAL=1 cargo test -p tunnel-lint --test real_tunnel_lint` (1 passed,
ports 19355-19357). All started processes were killed; `ss -ltn` showed no
listener left on 19350-19359 afterwards.

What the script does:
1. Builds release `tunnel-lint` and `tuner` (the workspace's real static HTTP server).
2. Takes the secrets-free copy of the VPS ingress config
   (`crates/tunnel-lint/tests/fixtures/vps-config-20261020.yml`), moves the 4 live VPS
   origins (8671, 8091, 8080, 8650) to 19350-19353 and the 21 dead VPS ports onto
   19354-19359 (round-robin), `chmod 666` like the VPS file.
3. Starts 4 real `tuner` origins on 19350-19353 and `curl`s each (200).
4. Lints live (TCP probes), stages, re-lints the staged copy, kills the 19353 origin,
   revives dead port 19354 with another `tuner`, replays the VPS `ss -ltnp` capture
   offline, and checks the credentials guard.

## Read-only VPS probes used for comparison (no files written on the VPS)

| Zagreb time | Probe | Result |
|---|---|---|
| 2026-10-20 20:35:20 +02:00 | `ss -ltnp` | 22 listeners; of the 25 distinct ingress ports, only 8671, 8091, 8080, 8650 listen |
| 2026-10-20 20:35:59 +02:00 | `GET 127.0.0.1:20241/metrics` (named tunnel) | `total_requests 84841`, `request_errors 50061` (59.0 %), `ha_connections 4` |
| 2026-10-20 20:35:59 +02:00 | `GET 127.0.0.1:20242/metrics` (quick tunnel) | `total_requests 21`, `request_errors 0` |
| 2026-10-20 20:35:59 +02:00 | `journalctl -u cloudflared --since "24 hours ago"` | 30 108 "Request failed … connection refused", 19 distinct origins |
| 2026-10-20 20:42:52 +02:00 | `getent ahosts tuner.galion.app` + `curl https://tuner.galion.app/` | no DNS record, "Could not resolve host" |
| 2026-10-20 20:42:24 +02:00 | public `GET` from the box | galion.studio 502, merlinelement.com 502, api.galion.app 502, one.galion.app 200, quick-tunnel URL 200 |

`tunnel-lint --ss` on the VPS capture gives exactly the set that `ss` and the
journal show: 21 closed ports; every origin in the journal's "connection
refused" list is among them, and none of the 5 OPEN rules appears there.

## Script output (abridged, full log on the box at `/workspace/swarm/evidence/tunnel-audit/real-run.txt`)

```

== 2026-10-20T20:45:59+02:00 build (release)
331936 /workspace/swarm/target/tunnel-audit/release/tuner
351040 /workspace/swarm/target/tunnel-audit/release/tunnel-lint

== 2026-10-20T20:45:59+02:00 ports 19350-19359 must be free
free

== 2026-10-20T20:45:59+02:00 box config from the VPS copy (live 8671/8091/8080/8650 -> 19350-19353, dead -> 19354-19359)

== 2026-10-20T20:45:59+02:00 start real tuner origins on 19350-19353
GET :19350 -> 200
GET :19351 -> 200
GET :19352 -> 200
GET :19353 -> 200

== 2026-10-20T20:45:59+02:00 lint live (expect exit 1: 33 closed routes on 6 ports, 5 open)
warn: /tmp/tunnel-audit-real.ZXJNvc/config.yml is world-writable (mode 666): any local user can reroute the tunnel
summary: 39 rules, 33 closed routes on 6 ports [19354, 19355, 19356, 19357, 19358, 19359], 34 findings
note: credentials-file is referenced; tunnel-lint did not read it
exit=1

== 2026-10-20T20:45:59+02:00 stage, diff, re-lint staged (expect exit 0)
tunnel-lint: staged config written to /tmp/tunnel-audit-real.ZXJNvc/staged.yml (not applied)
summary: 6 rules, 0 closed routes on 0 ports [], 0 findings
note: credentials-file is referenced; tunnel-lint did not read it
exit=0

== 2026-10-20T20:45:59+02:00 kill the tuner on 19353 (tuner.galion.app) and re-lint
38	tuner.galion.app		http://127.0.0.1:19353	19353	CLOSED	
exit=1

== 2026-10-20T20:45:59+02:00 revive dead port 19354 with a real tuner and re-lint
5	code.galion.app		http://127.0.0.1:19354	19354	OPEN	
15	reference.galion.app		http://localhost:19354	19354	OPEN	
21	ws3d.app		http://localhost:19354	19354	OPEN	
22	www.ws3d.app		http://localhost:19354	19354	OPEN	
23	workspace3d.app		http://localhost:19354	19354	OPEN	
24	www.workspace3d.app		http://localhost:19354	19354	OPEN	
29	machine.galion.app		http://localhost:19354	19354	OPEN	

== 2026-10-20T20:45:59+02:00 offline replay of the VPS capture (ss -ltnp 2026-10-20 20:35 +02:00)
summary: 39 rules, 33 closed routes on 21 ports [3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444, 5530, 7681, 8000, 8889], 33 findings
note: credentials-file is referenced; tunnel-lint did not read it
exit=1

== 2026-10-20T20:45:59+02:00 credentials guard (must refuse *.json, exit 2)
tunnel-lint: refusing to read tunnel.json: credentials are never read
exit=2

== 2026-10-20T20:45:59+02:00 gated Rust real test (GALION_REAL=1)
test real_binary_flags_closed_slot_ports ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

== 2026-10-20T20:45:59+02:00 teardown
PASS 2026-10-20T20:45:59+02:00
```

Closed/open table of the live run (first and last rows; the 33 `error: rule N`
lines are omitted above):

```
  1 OPEN   one.galion.app ^/tanga(/.*)?$          http://127.0.0.1:19350       
  2 OPEN   one.galion.app                         http://127.0.0.1:19351       
  3 OPEN   console.galion.app                     http://127.0.0.1:19352       
  4 OPEN   obsidian.galion.app                    http://127.0.0.1:19352       
  5 CLOSED code.galion.app                        http://127.0.0.1:19354       
  6 CLOSED jupyter.adriaticpadel.com              http://127.0.0.1:19356       
  7 CLOSED adriaticpadel.com                      http://127.0.0.1:19356       
  8 CLOSED www.adriaticpadel.com                  http://127.0.0.1:19356       
  9 CLOSED galion.app                             http://localhost:19356       
 38 OPEN   tuner.galion.app                       http://127.0.0.1:19353       
 39 SKIP   *                                      http_status:404              built-in
error: rule 5: code.galion.app -> port 19354: nothing listens (cloudflared answers 502)
error: rule 6: jupyter.adriaticpadel.com -> port 19356: nothing listens (cloudflared answers 502)
error: rule 7: adriaticpadel.com -> port 19356: nothing listens (cloudflared answers 502)
```

Staged diff excerpt (first hunk lines from the script):

```
--- /tmp/tunnel-audit-real.ZXJNvc/config.yml	2026-10-20 20:45:59.391639797 +0200
+++ /tmp/tunnel-audit-real.ZXJNvc/staged.yml	2026-10-20 20:45:59.439639797 +0200
@@ -28,106 +28,139 @@
-- hostname: code.galion.app
-  service: http://127.0.0.1:19354
-
-- hostname: jupyter.adriaticpadel.com
-  service: http://127.0.0.1:19356
-
-- hostname: adriaticpadel.com
-  service: http://127.0.0.1:19356
-- hostname: www.adriaticpadel.com
-  service: http://127.0.0.1:19356
-
```
