# Rust rewrite workstreams (started 2026-10-18 20:58 CEST +02:00)
Each stream works on its own branch `ws/<name>` in a separate git worktree under /workspace/oom-ws/<name>, own crates only.
- ws/one-desk-tuner: Galion One, galion-desk, tuner (phase 0 + Galion One Python helpers)
- ws/survival: guardian + healer + keep.sh/stack-boot/origin.py replacements
- ws/memory: memory-gateway Rust replacement
- ws/hermes: Hermes Rust port (hermes-rs) + upgrade staging
- queued: ws/speech (stt/tts/vision on CPU)
Integration into rust-v1 is done by Grok Bot.
