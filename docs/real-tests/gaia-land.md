<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life test: gaia CLI (swarm/gaia-land)

Rust only. The test is `crates/gaia/tests/real_cli.rs`, gated by `GALION_REAL=1`:

```
CARGO_INCREMENTAL=0 GALION_REAL=1 [GAIA_VPS=root@<vps>] \
  cargo test --release -j 3 -p gaia --test real_cli -- --nocapture --test-threads 1
```

It starts the real release `gaia` binary once per command. The CLI opens no socket and spawns nothing, so no port was used (slot 01 range 19010-19019 unused), and no process is left running. The live part runs only fixed read-only listings over ssh (`systemctl list-units`, `ss -ltnp`); nothing on the VPS was changed, built or restarted.

Binary sizes (release, stripped): x86_64 glibc 539,752 B; x86_64-unknown-linux-musl 628,536 B (static); aarch64-unknown-linux-musl 545,512 B (static). `cargo check` of all gaia crates for aarch64-linux-android and armv7-unknown-linux-musleabihf: OK.

## 1. Commands against fixtures (2026-10-20 20:48 CEST (+02:00))

```
status: "generated_at":"2026-10-20T20:45:12+02:00"
plan rollback: "outcome":"dry-run" "summary":"dry-run plan"
plan kill-switch: "outcome":"dry-run" "summary":"dry-run plan"
plan deploy-switch: "outcome":"dry-run" "summary":"deploy switch is plan-only; the owner runs deploy/switch.sh"
dry-run: "totals":{"systems":52,"planned":52,"destructive_refused":5,"destructive_acted":0}
dispatch teardown execute (no approval): "outcome":"refused"
simulate-faro --full-power: "summary":{"biomass_start":100000,"biomass_after_plague":0,"biomass_end":220000,"swarm_active":false,"hades":"contained","personas":13,"agents":7,"ticks":32,"lineage_records":87,"births":13}
  sweep: "totals":{"done":44,"dry_run":3,"refused":5,"rejected":0}
ownership: "crates_total":46 "system_crates":35 "systems_with_a_crate":27
view (fixture): "summary":{"units_seen":16,"units_running":12,"units_unmapped":0,"listeners":22,"listeners_unmapped":5,"systems_total":52,"systems_live":10,"systems_with_unit":11,"systems_with_crate":27}
bad input (plan no-such-system): exit 2
```

The fixture (`crates/gaia/tests/fixtures/vps-*-20261020.txt`) is a read-only capture of the VPS taken 2026-10-20 18:52-18:55 CEST (+02:00).
