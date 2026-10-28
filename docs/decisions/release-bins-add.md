<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Decisions: release-bins-add (wave-4 slot 337, closes D-325-2 on this branch)

2026-10-28 01:40 CET (+01:00) Europe/Zagreb.

Done: gaia-cortex (`llm-advisor`, as live `gaia-cortex-73583ab-llm`) and galion-console-serve are
in RELEASE_BINS with size budgets, guards and the byte proof (x86_64 musl 19/19 identical, see
docs/real-tests/release-bins-add.md). Nothing deployed.

Owner items:

- **D-337-1 land order.** This branch carries two merges (cortex-staging-land-check 186cd61c,
  console-v2-admin-fold c7cebb28) on top of release-repro-unify 6ffb2570. Options: (a) land
  release-repro-unify (D-325-1), cortex (D-293-4) and console-v2 on main-staging first, then
  rebase this branch to only the RELEASE_BINS commit (cleanest); (b) land this branch as the
  combined fold. Recommendation: (a).
- **D-337-2 release features in the recipe.** gaia-cortex release builds must pass
  `--features gaia-cortex/llm-advisor` (the guard fails without it). Enabling the advisor at run
  time still needs Mate's approval (`CORTEX_LLM_ENABLED`); the feature only compiles the curl
  transport in. Option: build a second no-llm cortex for hosts without approval.
- **D-337-3 nerve size headroom.** openmind-nerve on the cortex lineage is 2,020,224 B, 76,928 B
  under the 2 MiB cap (was 1,930,048 on main-staging). Next nerve growth fails size-budget.
  Options: trim (D-294-1 style) or a nerve-specific cap like hermes-gateway.
- **D-337-4 aarch64 byte proof** still blocked by the missing C cross compiler (D-318-3/D-325-3,
  slot 338 cross-cc-restore). aarch64 builds of the two new bins succeed (rust-lld, pure Rust).
- **F-337-1 (finding, not fixed).** gaia-cortex `/health` `"nerve": true` only means a nerve URL is
  configured (`engine.rs` `nerve_url.is_some()`); the box smoke shows `true` with nothing listening
  on the nerve port. Same blind-spot class as F-313-2. Suggest a real reachability/last-pull field.
