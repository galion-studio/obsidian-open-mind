<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-dist-arena-bank (wave 4 slot 336) — third-party

No new dependencies and no copied code. `crates/console-static/src/arena.rs` is a fresh
Rust port of the read-only `status()` behaviour of the in-house arena script
(`/workspace/galion-light/arena/arena.py`, read as text only, never run). It uses only
`std` and the existing in-repo `console_ui::json` reader/writer.
