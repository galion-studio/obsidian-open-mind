<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: gaia-hades-tokens (`gaia heal-tick` with hades-gate tokens)

Run on the box 2026-10-20 between 20:22:19 and 20:22:21 CEST (+02:00, Europe/Zagreb),
swarm slot 23, ports 19230 (openmind-nerve), 19231 (gaia-dummy), 19232 (kept free;
the wrong-plan token is minted for a plan naming it). Branch `swarm/gaia-hades-tokens`
= swarm/gaia-live 4fbbb7f + swarm/hades-approval ccaab42 + this work.

- Script: `scripts/real/gaia-hades-tokens.sh` (release build, x86_64; gaia 777,872 B,
  gaia-dummy 325,472 B, hades-gate 499,848 B, openmind-nerve 423,288 B).
- Rust version: `GALION_REAL=1 GAIA_NERVE_BIN=.../openmind-nerve cargo test --release -p gaia --test real_gaia_live`
  (ports 19233/19234): **1 passed** (healthy, no-token, forged, 3 valid restarts, replay,
  4th valid token capped and not consumed, 3 consumed nonces, nerve stamps +02:00/+01:00,
  no token in the nerve).
- Hermetic: `cargo test -p gaia --test heal_tick`: 10 passed (fake nerve, `/bin/true` as
  the "service", so the valid path ends in apply-failed after the approval is consumed).

**No VPS target.** Nothing here talks to the VPS, systemd or a port outside 19230-19239
(the 8420 case is refused by the scope guard before any probe). gaia heal-tick is not
deployed on the VPS, so there is no live service to compare against.

Keys: throwaway Ed25519 pair from `hades-gate keygen` (seed in a 0700 dir, mode 600) and a
second "attacker" pair for the forged token. Tokens from `hades-gate mint`, written mode 600.
No seed or token string is printed anywhere; the run ends with a grep proving no token
string or seed reached tick output, the nerve JSONL, `GET /events`, the gate ledger or the
restart state.

## Results

| # | Case | Token | Exit | Outcome / code | Nonces consumed after |
|---|------|-------|------|----------------|------------------------|
| 1 | dummy up | none | 0 | healthy | 0 |
| 2 | live port 8420 | valid-1 | 4 | refused at scope (before probe, token untouched) | 0 |
| 4 | dummy down | none | 4 | no-token (HADES dry-run plan only) | 0 |
| 5 | | other key | 4 | forged | 0 |
| 6 | | last sig nibble flipped | 4 | forged | 0 |
| 7 | | target gaia-other | 4 | wrong-scope (target) | 0 |
| 8 | | action rollback | 4 | wrong-scope (action) | 0 |
| 9 | | plan for port 19232 | 4 | wrong-scope (plan) | 0 |
| 10 | | iat now-400 s, ttl 300 s | 4 | expired | 0 |
| 11 | start argv `--note=secret` | valid signature over that plan | 4 | admin-safety (beats a valid token) | 0 |
| 12 | `hades-gate disable` (no token, no key) | valid-1 | 4 | disabled; then `enable` | 0 |
| 13.1 | | valid-1 | 0 | restarted 1/3, health verified | 1 |
| 14 | replay | valid-1 again | 4 | replayed | 1 |
| 13.2 | | valid-2 | 0 | restarted 2/3 | 2 |
| 13.3 | | valid-3 | 0 | restarted 3/3 | 3 |
| 15 | 4th within the hour | valid-4 | 5 | capped, retry in 3598 s, valid-4 NOT consumed | 3 |

33 nerve events, 33 JSONL lines on disk, 0 stamps without `+02:00`. All 3 processes
killed, ports 19230-19232 closed at the end.

## Output (verbatim)

```text

[2026-10-20 20:22:19 CEST (+02:00)] build (release, nice -n 10, -j 3, CARGO_TARGET_DIR=/workspace/swarm/target/gaia-hades-tokens)
777872 /workspace/swarm/target/gaia-hades-tokens/release/gaia
325472 /workspace/swarm/target/gaia-hades-tokens/release/gaia-dummy
499848 /workspace/swarm/target/gaia-hades-tokens/release/hades-gate
423288 /workspace/swarm/target/gaia-hades-tokens/release/openmind-nerve

[2026-10-20 20:22:19 CEST (+02:00)] admin side: hades-gate keygen (seed in /tmp/gaia-hades-tokens-real-20261020-202219/admin, mode 600, never printed); a second 'attacker' key
{"admin_fingerprint":"63d7f1729ebbbeaf","ok":true}
-rw------- /tmp/gaia-hades-tokens-real-20261020-202219/admin/admin.seed
-rw------- /tmp/gaia-hades-tokens-real-20261020-202219/gate/admin.pub
gate status: {"admin_fingerprint":"63d7f1729ebbbeaf","disabled":false,"mode":"dry-run","service":"hades-gate","ts":"2026-10-20T20:22:19+02:00"}

[2026-10-20 20:22:19 CEST (+02:00)] gaia heal-plan: the exact plan bytes the admin signs
{"target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b08b7a04d194df9aec862674600e0412dba17a6b8068600a6","admin_safety":"ok"}
gaia heal-apply plan v1 (box test service)
service gaia-dummy on 127.0.0.1:19231 (box test ports 19000-19999 only)
probe GET /health must answer status ok
start ["/workspace/swarm/target/gaia-hades-tokens/release/gaia-dummy","--port","19231"] detached in its own process group, stdin closed, no shell
at most 3 restarts of gaia-dummy per rolling hour
record the result on the nerve
hades-gate plan-hash: 29ae6c3a8d688b0b08b7a04d194df9aec862674600e0412dba17a6b8068600a6
{"plan_sha256":"b62e3eea4db42f10744cadc4fcdfcccb69bfe07dd86e3039b590884baecf8e18","admin_safety":"ok"}
{"plan_sha256":"833bb5de146e64e1612e638f1394c34544bb2d19c6ea27cc20138b5c4398df6f","admin_safety":"admin-safety: secret exposure refused (secret)"}

[2026-10-20 20:22:19 CEST (+02:00)] admin side: hades-gate mint (tokens written mode 600 to /tmp/gaia-hades-tokens-real-20261020-202219/tok, never printed)
{"token":"forged","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"bab7f4969d858b4158670bae23f9da21","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"wrong-svc","target":"gaia-other","plan_sha256":"29ae6c3a8d688b0b","nonce":"1933a75d6efc9ef3a0a9759f664ce75f","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"wrong-plan","target":"gaia-dummy","plan_sha256":"b62e3eea4db42f10","nonce":"0d405b19b51d14bc4df8723e854deddd","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"expired","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"5f32b872283fec9480b32d2948694a52","iat":"2026-10-20T20:15:39+02:00","exp":"2026-10-20T20:20:39+02:00"}
{"token":"unsafe","target":"gaia-dummy","plan_sha256":"833bb5de146e64e1","nonce":"a6b2dd9bbcfdbcf64fb223cfed3e69dd","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"valid-1","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"feb04e05ade45b630246f5c4fe49671b","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"valid-2","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"1c31f297c0e0392d73b3119dfaa98929","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"valid-3","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"9f073fed82283fff633f65ef27b0b516","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"valid-4","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b","nonce":"ad4d1c8f62fc933343a0435e56ebc34f","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"}
{"token":"wrong-action","action":"rollback","target":"gaia-dummy","nonce":"3b569e24d9f82a350953e46b7c8c1e25"}
-rw------- expired -rw------- forged -rw------- tampered -rw------- unsafe -rw------- valid-1 -rw------- valid-2 -rw------- valid-3 -rw------- valid-4 -rw------- wrong-action -rw------- wrong-plan -rw------- wrong-svc 

[2026-10-20 20:22:19 CEST (+02:00)] start the real openmind-nerve on 127.0.0.1:19230 and gaia-dummy on 127.0.0.1:19231
nerve GET /health: {"appended":0,"service":"openmind-nerve","status":"ok","ts":"2026-10-20T20:22:19+02:00","version":"0.1.0"}
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":1406224,"started":"2026-10-20T20:22:19+02:00"}

[2026-10-20 20:22:19 CEST (+02:00)] 1. dummy up: healthy, no token needed or touched
01-healthy       exit=0 outcome=healthy   code=-             consumed=0 detail=service is up; nothing to do

[2026-10-20 20:22:19 CEST (+02:00)] 2. scope guard: live stack port 8420 refused before any probe or token
02-live-port     exit=4 outcome=refused   code=-             consumed=0 detail=port 8420 is outside the allowed test range 19230-19239

[2026-10-20 20:22:19 CEST (+02:00)] 3. kill the dummy (pid 1406224, our own child)
dummy GET /health now: connection refused

[2026-10-20 20:22:19 CEST (+02:00)] 4. refusals (service down, nothing restarted, no token consumed)
04-no-token      exit=4 outcome=refused   code=no-token      consumed=0 detail=no approval token: HADES releases the dry-run plan only
05-forged        exit=4 outcome=refused   code=forged        consumed=0 detail=forged or tampered token: signature does not verify
06-tampered      exit=4 outcome=refused   code=forged        consumed=0 detail=forged or tampered token: signature does not verify
07-wrong-service exit=4 outcome=refused   code=wrong-scope   consumed=0 detail=token is scoped to a different target
08-wrong-action  exit=4 outcome=refused   code=wrong-scope   consumed=0 detail=token is scoped to a different action
09-wrong-plan    exit=4 outcome=refused   code=wrong-scope   consumed=0 detail=token is scoped to a different plan
10-expired       exit=4 outcome=refused   code=expired       consumed=0 detail=expired token

[2026-10-20 20:22:19 CEST (+02:00)] 5. admin safety first: a valid signature over an unsafe plan is still refused
11-admin-safety  exit=4 outcome=refused   code=admin-safety  consumed=0 detail=admin-safety: secret exposure refused (secret)

[2026-10-20 20:22:19 CEST (+02:00)] 6. hades-gate disable (no token, no key) beats a valid token; enable
{"disabled":true,"ok":true}
12-disabled      exit=4 outcome=refused   code=disabled      consumed=0 detail=HADES is disabled by the admin (disable file present)
{"disabled":false,"ok":true}

[2026-10-20 20:22:19 CEST (+02:00)] 7.1 valid token valid-1: restart 1 of 3
13.1-valid       exit=0 outcome=restarted code=approved      consumed=1 detail=restart 1/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":1406311,"started":"2026-10-20T20:22:19+02:00"}

[2026-10-20 20:22:19 CEST (+02:00)] kill the restarted dummy (pid 1406311)

[2026-10-20 20:22:20 CEST (+02:00)] 7.1b replay valid-1
14-replayed      exit=4 outcome=refused   code=replayed      consumed=1 detail=replayed token: nonce already consumed

[2026-10-20 20:22:20 CEST (+02:00)] 7.2 valid token valid-2: restart 2 of 3
13.2-valid       exit=0 outcome=restarted code=approved      consumed=2 detail=restart 2/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":1406332,"started":"2026-10-20T20:22:20+02:00"}

[2026-10-20 20:22:20 CEST (+02:00)] kill the restarted dummy (pid 1406332)

[2026-10-20 20:22:20 CEST (+02:00)] 7.3 valid token valid-3: restart 3 of 3
13.3-valid       exit=0 outcome=restarted code=approved      consumed=3 detail=restart 3/3 this hour; health verified
dummy GET /health: {"status":"ok","service":"gaia-dummy","pid":1406351,"started":"2026-10-20T20:22:20+02:00"}

[2026-10-20 20:22:20 CEST (+02:00)] kill the restarted dummy (pid 1406351)

[2026-10-20 20:22:21 CEST (+02:00)] 8. fourth valid token within the hour: capped, token NOT consumed
15-capped        exit=5 outcome=capped    code=-             consumed=3 detail=restart cap reached: 3 restarts of gaia-dummy in the last hour; next allowed in 3598 s
valid-4 nonce ad4d1c8f62fc933343a0435e56ebc34f in ledger as consumed: 0 (expected 0)
ledger: consumed nonces = 3 (expected 3); refusal/dry-run audit lines = 10

[2026-10-20 20:22:21 CEST (+02:00)] approval step of restart 1
{"step":"hades-plan","at":"2026-10-20T20:22:19+02:00","d":{"plan_sha256":"29ae6c3a8d688b0b08b7a04d194df9aec862674600e0412dba17a6b8068600a6","outcome":"dry-run","dry_run":true}}
{"step":"approval","at":"2026-10-20T20:22:19+02:00","d":{"admin_fingerprint":"63d7f1729ebbbeaf","token_sha256_16":"594e27d9b1dfbf41","decision":"WouldApply","code":"approved","approved":{"action":"heal-apply","target":"gaia-dummy","plan_sha256":"29ae6c3a8d688b0b08b7a04d194df9aec862674600e0412dba17a6b8068600a6","nonce":"feb04e05ade45b630246f5c4fe49671b","iat":"2026-10-20T20:22:19+02:00","exp":"2026-10-20T20:27:19+02:00"},"alpha":"owner","released":true}}

[2026-10-20 20:22:21 CEST (+02:00)] hades-gate ledger (no token strings; consumed nonces and refusal reasons)
{"ts":"2026-10-20T20:22:19+02:00","event":"dry-run","target":"gaia-dummy","reason":"no token","token_sha256_16":""}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"forged or tampered token: signature does not verify","token_sha256_16":"feae9a845e994899"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"forged or tampered token: signature does not verify","token_sha256_16":"85063df737f135cf"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"token is scoped to a different target","token_sha256_16":"7587544c2e20af64"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"token is scoped to a different action","token_sha256_16":"0ba903bb10ae6f23"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"token is scoped to a different plan","token_sha256_16":"5dfbfa74580606a6"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"expired token","token_sha256_16":"8cc0677389726dde"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"admin-safety: secret exposure refused (secret)","token_sha256_16":"2e2746a2f4a6dfe7"}
{"ts":"2026-10-20T20:22:19+02:00","event":"refused","target":"gaia-dummy","reason":"HADES is disabled by the admin (disable file present)","token_sha256_16":"594e27d9b1dfbf41"}
{"ts":"2026-10-20T20:22:19+02:00","event":"consumed","target":"gaia-dummy","nonce":"feb04e05ade45b630246f5c4fe49671b"}
{"ts":"2026-10-20T20:22:20+02:00","event":"refused","target":"gaia-dummy","reason":"replayed token: nonce already consumed","token_sha256_16":"594e27d9b1dfbf41"}
{"ts":"2026-10-20T20:22:20+02:00","event":"consumed","target":"gaia-dummy","nonce":"1c31f297c0e0392d73b3119dfaa98929"}
{"ts":"2026-10-20T20:22:20+02:00","event":"consumed","target":"gaia-dummy","nonce":"9f073fed82283fff633f65ef27b0b516"}

[2026-10-20 20:22:21 CEST (+02:00)] nerve GET /events?n=200 (ts, severity, kind, short data)
2026-10-20T20:22:19+02:00    info    service.probe    {"state":"up"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"scope"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"no-token"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"forged"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"forged"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"wrong-scope"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"wrong-scope"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"wrong-scope"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"expired"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"admin-safety"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"disabled"}
2026-10-20T20:22:19+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:19+02:00    info    gaia.heal.approved    {"nonce":"feb04e05ade45b630246f5c4fe49671b"}
2026-10-20T20:22:19+02:00    info    gaia.heal.applied    {"restarts_last_hour":1,"pid":1406311,"health_verified":true,"nonce":"feb04e05ade45b630246f5c4fe49671b"}
2026-10-20T20:22:20+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:20+02:00    warn    gaia.heal.refused    {"stage":"hades-approval","code":"replayed"}
2026-10-20T20:22:20+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:20+02:00    info    gaia.heal.approved    {"nonce":"1c31f297c0e0392d73b3119dfaa98929"}
2026-10-20T20:22:20+02:00    info    gaia.heal.applied    {"restarts_last_hour":2,"pid":1406332,"health_verified":true,"nonce":"1c31f297c0e0392d73b3119dfaa98929"}
2026-10-20T20:22:20+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:20+02:00    info    gaia.heal.approved    {"nonce":"9f073fed82283fff633f65ef27b0b516"}
2026-10-20T20:22:20+02:00    info    gaia.heal.applied    {"restarts_last_hour":3,"pid":1406351,"health_verified":true,"nonce":"9f073fed82283fff633f65ef27b0b516"}
2026-10-20T20:22:21+02:00    warn    service.probe    {"state":"down"}
2026-10-20T20:22:21+02:00    error    gaia.heal.capped    {"restarts_last_hour":3,"retry_after_s":3598,"token_consumed":false}
nerve events: 33; JSONL lines on disk: 33
nerve stamps without +02:00: 0
restart ledger: {"service":"gaia-dummy","ts":"2026-10-20T20:22:19+02:00","pid":1406311} {"service":"gaia-dummy","ts":"2026-10-20T20:22:20+02:00","pid":1406332} {"service":"gaia-dummy","ts":"2026-10-20T20:22:20+02:00","pid":1406351} 

[2026-10-20 20:22:21 CEST (+02:00)] token / seed leak check
ok: no token string or seed in tick output, nerve log, gate ledger or state

[2026-10-20 20:22:21 CEST (+02:00)] teardown
port 19230 closed
port 19231 closed
port 19232 closed

[2026-10-20 20:22:21 CEST (+02:00)] done; evidence in /tmp/gaia-hades-tokens-real-20261020-202219
```

## Licences

`cargo tree -p gaia -e normal` closure joined with `cargo metadata` licences
(2026-10-20 20:25 CEST, +02:00):

```text
block-buffer 0.10.4 MIT OR Apache-2.0
cfg-if 1.0.5 MIT OR Apache-2.0
chrono 0.4.45 MIT OR Apache-2.0
chrono-tz 0.10.4 MIT OR Apache-2.0
cpufeatures 0.2.17 MIT OR Apache-2.0
crypto-common 0.1.7 MIT OR Apache-2.0
curve25519-dalek 4.1.3 BSD-3-Clause
digest 0.10.7 MIT OR Apache-2.0
ed25519 2.2.3 Apache-2.0 OR MIT
ed25519-dalek 2.2.0 BSD-3-Clause
generic-array 0.14.7 MIT
iana-time-zone 0.1.65 MIT OR Apache-2.0
itoa 1.0.18 MIT OR Apache-2.0
memchr 2.8.3 Unlicense OR MIT
num-traits 0.2.19 MIT OR Apache-2.0
phf 0.12.1 MIT
phf_shared 0.12.1 MIT
serde_core 1.0.229 MIT OR Apache-2.0
serde_json 1.0.154 MIT OR Apache-2.0
sha2 0.10.9 MIT OR Apache-2.0
signature 2.2.0 Apache-2.0 OR MIT
siphasher 1.0.4 MIT OR Apache-2.0
subtle 2.6.1 BSD-3-Clause
typenum 1.20.1 MIT OR Apache-2.0
zeroize 1.9.1 Apache-2.0 OR MIT
zmij 1.0.23 MIT
```

## Ported to Rust (2026-10-21, +02:00)
`scripts/real/gaia-hades-tokens.sh` was replaced on swarm/realscript-port-1 by the gated Rust test `crates/galion-realtest/tests/real_sh_gaia_hades_tokens.rs` (same checks; run with `GALION_REAL=1`). The evidence above is from the shell version; the Rust run is in `docs/real-tests/realscript-port-1.md`.
