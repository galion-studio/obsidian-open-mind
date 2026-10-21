<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: POSEIDON token auth (slot 205, Gaia S4.1)

Box only. No LAN bind on the VPS, nothing deployed; the VPS was only read
(GET + `ss` + `systemctl list-units`). Secrets in tests are fixed test strings
(`...-not-real`) and are asserted never to appear in any output.

## Box real test (release binary, GALION_REAL=1)

`GALION_REAL=1 cargo test -p openmind-queue --test real_auth -- --nocapture`
(also passes with `POSEIDON_BIN=.../x86_64-unknown-linux-musl/release/openmind-queue`).
Non-loopback test bind = the box's own LAN-side address 172.30.0.2 (found by
UDP route lookup, no packet sent), ports 20753 (loopback) and 20754 (LAN).

```
2026-10-21T19:47:45+02:00 bin /workspace/swarm/target/poseidon-auth/release/openmind-queue lan 172.30.0.2:20754 lo 127.0.0.1:20753
2026-10-21T19:47:45+02:00 refused [lan without --token-file]: exit Some(2): openmind-queue: start: refusing to bind non-loopback 172.30.0.2:20754 without --token-file (no unauthenticated LAN bind)
2026-10-21T19:47:45+02:00 refused [lan without --allow-remote]: exit Some(2): openmind-queue: start: refusing to bind non-loopback 172.30.0.2:20754 without --allow-remote
2026-10-21T19:47:45+02:00 refused [0644 token file]: exit Some(2): openmind-queue: start: token file /tmp/poseidon-real-auth-2533368/loose has mode 0644; need 0600 or 0400
2026-10-21T19:47:45+02:00 refused [--token-on-loopback without file]: exit Some(2): openmind-queue: start: --token-on-loopback needs --token-file
2026-10-21T19:47:45+02:00 loopback no-token CLI submit+worker ok; auth {"mode":"token-for-remote","secret_configured":true,"token_on_loopback":false,"verdicts":{"invalid":0,"loopback":5,"missing":0,"no_secret":0,"token":0}}
2026-10-21T19:47:45+02:00 lan no token: exit 1 `401 {"detail":"missing","error":"unauthorized","hint":"non-loopback requests need Authorization: Bearer <token>"}`
2026-10-21T19:47:45+02:00 lan wrong token: exit 1 `401 {"detail":"invalid","error":"unauthorized","hint":"non-loopback requests need Authorization: Bearer <token>"}`
2026-10-21T19:47:45+02:00 lan token CLI: 5 submits 201, worker drained 6 lines
2026-10-21T19:47:46+02:00 burst 600 wrong tokens: 401 x600 in 56 ms; stats auth {"mode":"token-for-remote","secret_configured":true,"token_on_loopback":false,"verdicts":{"invalid":601,"loopback":0,"missing":3,"no_secret":0,"token":17}}
2026-10-21T19:47:46+02:00 cleanup: servers killed, temp dir removed
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
```

`missing 3` = CLI `get /healthz` without token, worker "evil" without token
(exits on 401 instead of spinning), and nothing else; `invalid 601` = 600 burst + 1
wrong-token CLI submit. After every refused POST the queue depth stayed 0.
After the run: no listener on 20750-20759, temp dir removed.

## In-process tests (`tests/server_auth.rs`, always run)

- start-up refusals: LAN bind without `--allow-remote`; with it but no
  `--token-file` (PermissionDenied); `--token-on-loopback` without file; token
  file modes 0644/0640/0606; missing file. Nothing bound afterwards.
- loopback 127.0.0.1:20750 with a token file: submit/lease/ack with no token,
  a wrong token from loopback is ignored (unchanged behaviour).
- LAN bind <box IP>:20751: 12 paths x 7 bad credentials (none, wrong,
  Basic, secret+1 char, secret-1 char, empty Bearer, upper-cased) = 84 x 401
  each with `WWW-Authenticate: Bearer`; duplicate Authorization = 400; no
  refused request changed state (depth 0, cap unset); counters match; with
  token the full job cycle works; lower-case `bearer` accepted; loopback port
  not listening.
- `--token-on-loopback`: loopback without/with wrong token 401, with token 200.

## VPS read-only comparison (2026-10-21 19:43:47 +02:00)

- live `openmind-queue.service` active, listening on 127.0.0.1:19120 only
  (`ss -ltnp`); `GET /healthz` 200, `GET /stats` 200 (old build: no `auth`
  block); a GET to the VPS public address on :19120 gets no connection (000).
  The new default (no `--token-file`) is byte-identical in routing for
  loopback, so swapping the binary would not change the live unit.
- nerve 127.0.0.1:8670 `/state` (cycle 3, recently restarted): flags :19120
  among "4 loopback listener(s) no monitored unit owns: :8672 :8674 :18800
  :19120 ... process not readable from /proc", although
  `openmind-queue.service` owns it -> nerve owner-mapping gap for this unit
  (not this slot's change; planner note).
- cortex 127.0.0.1:8674 `/health` 200 (live, dry-run).
