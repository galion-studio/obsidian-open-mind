<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# survival-refresh-writer (wave 4 slot 389) — third-party

No new dependencies and no copied third-party code.

- `crates/console-static/src/survival.rs` is new, `std` only (threads, `std::process::Command`,
  file mtime), plus the in-repo `console_ui::pages::p2::zagreb` clock and `console_ui::json`
  (tests). `console-static` stays dependency-free apart from `console-ui`.
- `crates/galion-survival` is imported unchanged (then `cargo fmt`) from our own branch
  `swarm/survival-s47-install` b7fbbc7 (slot t276u); it is the binary already installed on the
  VPS. Its deps (`serde`, `serde_json`, in-repo `zagreb-time` → `chrono`, `chrono-tz`) are
  existing workspace deps, all MIT OR Apache-2.0 (Cargo.lock unchanged apart from the new
  workspace member entry).
