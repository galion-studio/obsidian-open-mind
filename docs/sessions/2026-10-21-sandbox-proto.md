<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Session 2026-10-21 06:34–06:58 CEST (+02:00) — galion-box prototype (wave-4 slot 144)

- Built `crates/galion-box`: rootless namespace sandbox launcher (design: docs/sandbox/galion-box.md).
- Real tests on the VPS as a throwaway user (docs/real-tests/galion-os-sandbox-proto.md):
  x86_64 10/10 applicable pass; aarch64 under qemu-user 3/3 applicable pass, namespaces impossible
  under qemu-user, launcher fails closed.
- Four bugs found by the real runs and fixed (pid-ns PDEATHSIG, LXC read-only remount, qemu/threads,
  20 ms start-up poll).
- Owner decisions D-S1..D-S4 written up; nothing decided for Slavik. No phone access.
