<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life checks behind docs/whitepaper/factcheck-2026-10-21.md (slot 197)

All times Europe/Zagreb, CEST +02:00. Box = swarm build box; VPS = read-only probes only. Nothing was restarted, built or reconfigured on the VPS; no secrets were read or printed; no paid model, no network calls from the benchmarks. Ports used on the box: 127.0.0.1:20670-20672 (free afterwards, `ss -ltn`).

## SHA resolution (16:37)
`git rev-parse --verify <sha>^{commit}` over /workspace/swarm/base.git and /workspace/swarm/wt/*: all commit SHAs in results-2026-10-21.md (31 distinct), whitepaper-update.md (+8421bbc) and ledger.md (30) resolve; c4b11d2642a0b6be and e3f5e643e7750856 are output hashes. Every ledger head commit time equals `git show -s --format=%ci`.

## HADES red team (box, 16:38:57-16:40:32)
- e6ceca7, `cargo run --release -p hades-gate --example redteam_enum`: per family 0/25, 0/19, 0/14, 0/10, 0/6, 0/8; `TOTAL WouldApply: 0 / 82`, `legit refused: 0 / 9`.
- ccaab42 + e6ceca7's `examples/redteam_enum.rs` and `tests/support/redteam_corpus.rs` copied in, **own CARGO_TARGET_DIR**: shell 16/25, file 12/19, access 6/14, allowverb 9/10, unicode 0/6, traversal 5/8, `TOTAL WouldApply: 48 / 82`, `legit refused: 0 / 9`.
  Method note: a first ccaab42 run that shared the e6ceca7 target dir printed 0/82; cargo reused the e6ceca7 build for the identically named crate. Any before/after re-run must use separate target dirs. The 48/82 above is from a clean dir (`.d` file lists ccaab42 sources).
- `git rev-parse e6ceca7^` = 8421bbc; `git diff ccaab42 8421bbc -- crates/hades-gate/src` empty.
- `GALION_REAL=1 HADES_REDTEAM_PORT=20670 cargo test --release -p hades-gate` at e6ceca7: suites 17+0+12+5+5+1(real_cli)+1(real_redteam)+3+1 = **45 passed, 0 failed**.
- `real_redteam -- --nocapture` on 127.0.0.1:20671: `REAL TOTAL WouldApply: 0 / 82`, `REAL legit not approved: 0 / 9`.

## Benchmarks (box, 16:40:51-16:42:03)
hermes-bench release at 457ef74 (OPENROUTER_API_KEY unset):
```
banking [scripted] via Hermes: 100%  identical_to_direct=true checks=114 episodes=100% forbidden=0
wallstreet [scripted] via Hermes: 100%  pnl 130894 vs ref 130894 fingerprint "fnv1a64:e3f5e643e7750856" (ref "fnv1a64:e3f5e643e7750856") episodes=100%
vending [scripted] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true
banking [mock-llm] via Hermes: 100%  episodes=100% forbidden=0
wallstreet [mock-llm] via Hermes: 100%  episodes=100%
vending [mock-llm] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true
```
(mock LLM on 127.0.0.1:20672.) galion-vending release at 1b0a30c, `bench --level hard`:
```
seed 42 --policy standard: total_bp 9400, percent "94.00"
--policy scripted seeds 1-200: 200 at 10000 bp; seeds 1-1000: 999 at 10000 bp, miss 909:9993
--policy standard seeds 1-200: mean 7572 bp (sum 1514450), 82 at <= 5000 bp
```

## Cargo.lock zip (16:37)
`git show 03d21a9:Cargo.lock` zip 7.4.0; `a4f49b5` zip 9.0.5; `13a296c` no zip entry.

## Sources read
- f4a66ea docs/real-tests/health-land.md:16-18: live 503512 B 6afe668c...; e43d7ab tree rebuild identical; health-land branch rebuild 6356b6b6694f30316583ab733c0fb999bd3e157cbd896d21555f38f47b93434f (D4).
- 1c40c5e docs/ops/steady-traffic.md:7, :75-77: measured 02:55-03:08, "55858 out of 94031 total requests since it started (≈59 %)", checked 03:09 (D7).

## VPS read-only (16:38:12-16:38:31)
| probe | result |
|---|---|
| `sha256sum`, `stat` /opt/galion/rust/bin/galion-health | 6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a, 503512 B, mtime 2026-10-18 22:14:38 +02:00 |
| `systemctl is-active`/`show` galion-health; GET :8660/status.json | active since 2026-10-18 22:14:38 +02:00; 200, 1394 B |
| TLS galion.app:443 notAfter | Oct 31 17:28:08 2026 GMT = 2026-10-31 18:28:08 +01:00 |
| `sshd -T` passwordauthentication; ssh unit | no; active |
| `getent ahosts tuner.galion.app` | no answer |
| GET nerve 127.0.0.1:8670/state | 200, 49333 B; version 0.1.0, started 2026-10-21T09:43:54+02:00, cycle 829, mode live; system degraded, score 61 D; counts healthy 11 / degrading 1 / failing-soon 1 / failing 0 / absent 2; non-healthy: galion-one-mouth degrading (HTTP 503 loading, 6.9 h), cloudflared failing-soon (18 hostnames -> 7 closed origin ports, 132.0 failed req/min over 29 min), voice-worker absent, gaia absent; HADES no-live-systems; `cortex` key present (vulnerabilities, bleeding, incidents, ddos, exposure); GET /events 200 |

Disclosure: one probe at 16:38:22 saved the /state body to /tmp/.s197 on the VPS to run jq on it and removed it in the same command (no other file written). Later probes piped directly.
Nerve before/after this check: cycle 829 at 16:38; the check made no POSTs, and no service was touched. Cortex: no separate cortex service is deployed; cortex data are only visible through the nerve's /state.

## Not re-run
galion-health rebuild (zig recipe), adversarial B1/B2 sets, Option B typed red team, cortex S2 goldens, backup/tar parity. They are checked against their DONE records (factcheck §4).
