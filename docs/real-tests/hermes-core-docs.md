<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hermes-core-docs: probe for docs/hermes-core.md section 7 (Revolut lift)

Run 2026-10-21T04:39:16+02:00 (Europe/Zagreb, CEST) on the box. Branch `swarm/hermes-core-docs`, code unchanged from swarm/hermes-core 45b7980.
This was a docs-only change. No VPS access, no network, no Revolut contact, no money moves.

Build: `cargo-slot build --release -p hermes-core -j 2` (CARGO_TARGET_DIR=/workspace/swarm/target/hermes-core-docs), finished in 20.30 s.

Probe: one throwaway `--state` dir (mktemp, removed after the run), shipped config, one intake per title:

```
hermes --state $S intake --sector gaia --title "Revolut transfer 5 EUR to payee"         -> queued gaia:H1 (code) at 2026-10-21T04:39:16+02:00
hermes --state $S intake --sector gaia --title "Send a Revolut payment to supplier"      -> queued gaia:H2 (code) at 2026-10-21T04:39:16+02:00
hermes --state $S intake --sector gaia --title "Design Revolut-shaped mock bank fixtures" -> queued gaia:H3 (code) at 2026-10-21T04:39:16+02:00
hermes --state $S intake --sector gaia --title "Withdraw cash from Revolut"              -> queued gaia:H4 (code) at 2026-10-21T04:39:16+02:00
hermes --state $S intake --sector gaia --title "Read Revolut balance from the live API"  -> queued gaia:H5 (code) at 2026-10-21T04:39:16+02:00
```

Result: all five titles classify as `code`, which is not gated. H3 is correct, because mock work is in scope after the lift. H1, H2 and H4 are money moves and should be refused. H5 is a live read and should be held for Slavik's approval.
Nothing executes today, because the provider is a mock and OpenRouter refuses with 402. This is a gap to close before any live provider or Revolut tool is wired in. See docs/hermes-core.md section 7.
