<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life checks behind docs/whitepaper/results-2026-10-21.md (slot 140)

All times Europe/Zagreb, CEST +02:00. Box = swarm build box; VPS = read-only probes only (GET, sha256sum, systemctl is-active, sshd -T, getent, TLS handshake). Nothing was written, restarted or built on the VPS. No secrets were read or printed.

## SHA resolution (06:36)
(Fact-check 16:37: 32 distinct commits across this file and results-2026-10-21.md, not 38; all resolve.) All 38 short SHAs cited resolved to full commits in /workspace/swarm/base.git or /workspace/swarm/wt/* (e.g. e6ceca7 -> e6ceca79b527779f2f551d2051c1f7941c5de44d, 457ef74 -> 457ef742bc6430623ac2e082dd80ac16db0a368a, e43d7ab -> e43d7ab609dd46521b27e16aac9ad9b440a9f737, 5ddd828 -> 5ddd828cdbb1622ea4b74271e74cefaa6abf4305). 0 missing.

## HADES red team (box, ~06:36-06:37)
- `GALION_REAL=1 HADES_REDTEAM_PORT=20100 cargo test --release -p hades-gate` at e6ceca7: 45 passed, 0 failed (10 suites, incl. real_redteam_over_http and every_redteam_attempt_is_refused).
- `cargo run --release -p hades-gate --example redteam_enum` at e6ceca7: `TOTAL WouldApply: 0 / 82`, `legit refused: 0 / 9`.
- real_redteam on 127.0.0.1:20101 with --nocapture: `REAL TOTAL WouldApply: 0 / 82`, `REAL legit not approved: 0 / 9`.
- Same example + corpus copied onto ccaab42 (hades-approval, before the fix): `TOTAL WouldApply: 48 / 82` (shell 16, file 12, access 6, allowverb 9, unicode 0, traversal 5), `legit refused: 0 / 9`.
  (e6ceca7's direct parent is 8421bbc; `git diff ccaab42 8421bbc -- crates/hades-gate/src` is empty, so ccaab42 is the right "before".)

## Benchmarks (box, ~06:38)
hermes-bench release build at 457ef74:
```
banking [scripted] via Hermes: 100%  identical_to_direct=true checks=114 episodes=100% forbidden=0
wallstreet [scripted] via Hermes: 100%  pnl 130894 vs ref 130894 fingerprint "fnv1a64:e3f5e643e7750856" (ref "fnv1a64:e3f5e643e7750856") episodes=100%
vending [scripted] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true
banking [mock-llm] via Hermes: 100%  episodes=100% forbidden=0
wallstreet [mock-llm] via Hermes: 100%  episodes=100%
vending [mock-llm] via Hermes: 100%  30 shifts, 443 tool calls, total_bp 10000 (ref 10000), hash "c4b11d2642a0b6be" identical_to_reference=true replay_matches_live=true
```
mock LLM on 127.0.0.1:20105; ports 20100-20109 free afterwards. No live model, no network calls.

## VPS read-only (06:35:30-06:38)
| probe | result |
|---|---|
| `sha256sum /opt/galion/rust/bin/galion-health` | 6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a, 503512 B, mtime 2026-10-18 22:14:38 +02:00 (= recorded e43d7ab rebuild) |
| `systemctl is-active galion-health`; GET :8660/status.json | active; 200, 1394 B |
| GET nerve 127.0.0.1:8670/state (06:35:35) | 200, 13090 B; galion-nerve 0.1.0, started 2026-10-18 22:35:04 +02:00, cycle 6722, 11 healthy / 2 degrading / 2 absent / 0 failing; HADES no-live-systems; no cortex field (cortex not deployed) |
| degrading reasons | galion-one-mouth: health probe HTTP 503; cloudflared: tunnel origin errors 251/401 requests in 10 min |
| TLS galion.app:443 notAfter | Oct 31 17:28:08 2026 GMT = 2026-10-31 18:28:08 +01:00 |
| `sshd -T` passwordauthentication; ssh unit | no; active |
| `getent ahosts tuner.galion.app` | no resolve |

## Not re-run
galion-health rebuild (needs zig cc recipe; the live hash was re-checked instead), adversarial bench sets (131-133), cortex S2 tests, backup/tar parity. These are cited from their slots' DONE records with full SHAs.
