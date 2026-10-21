<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# integration-3: real-life test record

Box only, 2026-10-21 01:55-02:02 CEST (+02:00), head after F3 (75daf4a),
target dir `/workspace/swarm/target/integration-3`. Each suite builds,
starts its real binaries on loopback, probes them and tears them down. Ports
are each suite's own (slot ranges of the branch that wrote it); the
hades-gate red team was moved to this slot's range with
`HADES_REDTEAM_PORT=19700`. No VPS access, no paid calls (hermes-bench runs
scripted + mock LLM only; no OpenRouter key in the environment).
No processes left behind (checked with `ps` afterwards).

| Suite | Command (`GALION_REAL=1 cargo-slot test ...`) | Result | Evidence |
|---|---|---|---|
| gaia heal-tick (hades tokens, plan v2) | `--release -p gaia --test heal_tick` | 10/10 | valid token restarts once, replay refused, forged/tampered/expired refused, cap checked before the token is consumed |
| gaia live heal end to end | `-p gaia --test real_gaia_live` (GAIA_NERVE_BIN = release openmind-nerve) | 1/1 | real nerve :19233 + gaia-dummy :19234: kill, three signed v2-plan restarts, no-token/forged/replay refused, fourth capped, all read back from the nerve |
| gaia CLI | `--release -p gaia --test real_cli` | 2/2 | dry-run 52 systems, 52 planned, 5 destructive refused, 0 acted; ownership crates_total 63, system_crates 51, systems_with_a_crate 29 |
| hades-gate red team | `--release -p hades-gate --test real_redteam` | 1/1 | real `hades-gate serve` on 127.0.0.1:19700: red-team plans 403 `admin-safety`, legit plans not approved 0/9 |
| build-info `--version` | `--release -p galion-common --test real_version` | 1/1 | 10 stamped binaries (incl. room, tuner) answer instantly |
| galion-health | `--release -p galion-health --test real_health` | 1/1 | |
| galion-backup parity | `--release -p galion-backup --test real_parity` | 1/1 (+2/2 in the second target) | Rust vs reference parity |
| cortex sensor | `--release -p openmind-nerve --test real_cortex_sensor` | 1/1 | |
| hermes-bench | `--release -p hermes-bench --test real_hermes_bench` | 1/1 | B1/B2/B3 through hermes-rs tool registries, scripted + mock LLM |
| galion-deploy backup | `-p galion-deploy --test real_backup` | 1/1 | debug profile: the test needs the `fake_galion_backup` example, which `--release` does not build (first `--release` attempt failed on exactly that assertion) |

Not run: `galion-common --test real_migrate` (expected to differ for
galion-desk, room, link-watch, see docs/merges/integration-3.md B),
`galion-deploy --test real_rehearsal`/`real_build`, the older
`scripts/real/*.sh`.
