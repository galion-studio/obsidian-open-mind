# Rust rewrite workstreams (started 2026-10-18 20:58 CEST +02:00)
Each stream works on its own branch `ws/<name>` in a separate git worktree under /workspace/oom-ws/<name>, own crates only.
- ws/one-desk-tuner: Galion One, galion-desk, tuner (phase 0 + Galion One Python helpers)
- ws/survival: guardian + healer + keep.sh/stack-boot/origin.py replacements
- ws/memory: memory-gateway Rust replacement
- ws/hermes: Hermes Rust port (hermes-rs) + upgrade staging
- queued: ws/speech (stt/tts/vision on CPU)
Integration into rust-v1 is done by Grok Bot.

## ws/gaia — Gaia, the universal creator (side project)
Branch `ws/gaia`, folder `gaia/`. Creates agents, personas, tools and projects from seed files; selection by the repo checks. Follows UNIVERSAL-CPU.md. Status (2026-10-18 21:50 CEST +02:00): M1 done. HZD-GAIA design doc (`gaia/docs/gaia-design.md`), `crates/gaia-core` plus nine subordinate-function crates (`gaia-hades`, `gaia-minerva`, `gaia-apollo`, `gaia-hephaestus`, `gaia-artemis`, `gaia-poseidon`, `gaia-aether`, `gaia-demeter`, `gaia-eleuthia`) and the `gaia` CLI (`simulate-faro [--full-power]`). Every stack system is assigned to exactly one function. HADES is plan-only and approval-gated. 71 tests; tier 1 and 2 OK. Next: seed to candidate to selection loop.
