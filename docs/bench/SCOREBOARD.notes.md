<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
## Notes (appended by `bench-scoreboard --notes docs/bench/SCOREBOARD.notes.md`)
- Standard rows B1-B3: SHAs are the sibling heads at the first fold-in (bench-scoreboard, 2026-10-20T23:20:00+02:00).
  Adversarial / hard rows: SHAs are the heads of swarm/bench-banking-adv (44e5c61), swarm/bench-ws-adv (2ecfc19) and
  swarm/bench-vending-adv (1b0a30c), merged onto swarm/bench-hermes 457ef74 in swarm/bench-scoreboard-adv
  (wave-4 slot 173, 2026-10-21 +02:00). Every score JSON shown is committed on this branch under `docs/bench/`.
- 100 % means 100 % of the mock / paper / simulation rubric in `plans/benchmarks.md`, not live behaviour.
  B1's read-only live reachability probe (B1b) is blocked on the phone link and an owner decision.
  B3 and B3-HARD are driven by a scripted operator (a ceiling); via Hermes they run through the hermes-rs agent loop
  with a scripted or mock-LLM provider. No real model was used anywhere (Hermes's paid model stays off, H1).
- The live VPS hermes-gateway (`/opt/galion/rust/bin/hermes-gateway-aa77864`) has no banking tools
  (`payee_add`, `payment_draft`, `memo_flags`, `refused_scope`: 0 strings), and there is no paper-broker or vending
  service on the VPS, so the adversarial sets cannot be replayed against the real server today; see
  `docs/real-tests/bench-scoreboard-adv.md` for the read-only probes.
- No live bank or broker call, no money moved (B1/B1-ADV JSON: `money_moves: 0`, mock `forbidden_total: 0`;
  B2/B2-ADV: `live_broker: false`).
