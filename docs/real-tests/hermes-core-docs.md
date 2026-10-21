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

## Re-check 2026-10-21T04:45+02:00 (resumed slot 127, standing rule no_skip_extensive_testing)

Box, same branch (code still 45b7980):
- `cargo-slot test -p hermes-core -j 2`: 42 passed, 0 failed, 0 ignored (30 unit + 6 + 5 + 1 integration).
- Release rebuild 21.16 s; probe repeated at 2026-10-21T04:45:38+02:00 in a throwaway `--state` dir (removed): "Revolut transfer 5 EUR to payee", "Withdraw cash from Revolut", "Read Revolut balance from the live API", "Design Revolut-shaped mock bank fixtures" -> all `queued ... (code)`. Gap confirmed: `src/action.rs` has 0 money/revolut keywords; `BRIEF_RULES` (`src/provider.rs` line 36) still says "no Revolut, no real money".

VPS, read-only GETs only, 2026-10-21T04:45:46+02:00:
- Live nerve 127.0.0.1:8670: `/healthz` 200, `/state` 13,139 B, mode `live`; component statuses 15 healthy, 6 degrading, 3 absent, 2 no-live-systems. The live /state has no `hermes-core`, `revolut` or `cortex` entries (cortex is not live yet; it shows only on the nerve-shadow unit), and `hermes-rs` appears 4 times.
- hermes-rs 127.0.0.1:8642 `/health` 200, unit active. No hermes-core binary in /opt/galion/rust/bin, so hermes-core is not deployed. No Revolut path exists on the live server, and nothing on the VPS was changed.
