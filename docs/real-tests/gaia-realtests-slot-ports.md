<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Slot 436 gaia-realtests-slot-ports (+ owner t429u Q3 bounds widening)

Date: 2026-10-30 08:55 +01:00. Base: gaia-test-ports-override 5f9ff96f. Test/guard only, NO deploy.

## Changes
- `gaia_live::heal::TEST_PORTS_OVERRIDE_BOUNDS` widened 19000-22999 -> 19000-23999 (owner t429u Q3,
  Mate 08:32 +01:00). Production range `TEST_PORTS` stays 19000-19999. Test 423 edge moved:
  23000-23009 is now accepted, 24000-24009 refused (`bounds_edge_is_23999_after_widening`).
- Converted to the range in force (`test_ports()`): real_gaia_live (ports check), heal_tick
  (allowed range, free port, heal-plan CLI block), heal_loop (edge port), gaia-live tests/heal.rs
  scope tests. real_gaia_loop, real_gaia_poseidon, real_poseidon_handle, real_test_ports already
  had REAL_*_PORT_BASE blocks.
- repo-check rule `gaia-ports`: flags hard-coded 19000-19999 ports in crates/gaia*/tests/real_*.rs
  (skips comments, Block::new / base_from_env / const DEFAULT_ lines, range names, allow marker).

## Evidence (box)
- Default env: clippy -D warnings (gaia, gaia-live, repo-check, openmind-nerve all targets) clean;
  cargo test -p gaia -p gaia-live -p repo-check all green; repo-check full tree PASS (0 errors).
- GALION_REAL=1 GAIA_TEST_PORTS=23060-23069, all PORT_BASE vars 23060 (loop nerve 23069):
  real_gaia_live 1/1, real_gaia_loop 1/1, real_gaia_poseidon 1/1, real_poseidon_handle 3/3,
  real_test_ports 1/1, heal_tick 13/13, heal_loop 11/11, gaia-live heal 12/12, live 5/5,
  test_ports_override 8/8.
- Negative control: `GAIA_TEST_PORTS=23060-23069` without GALION_REAL -> exit 2 "needs
  GALION_REAL=1"; `GAIA_TEST_PORTS=24000-24009` with GALION_REAL -> exit 2 "outside the override
  bounds 19000-23999"; with GALION_REAL in-block heal-plan -> exit 0 (plan names 23061).
- real_cli not run (ssh leg to a host; it has no ports).
