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
status: "generated_at":"2026-10-20T20:49:41+02:00"
plan rollback: "outcome":"dry-run" "summary":"dry-run plan"
plan kill-switch: "outcome":"dry-run" "summary":"dry-run plan"
plan deploy-switch: "outcome":"dry-run" "summary":"deploy switch is plan-only; the owner runs deploy/switch.sh"
dry-run: "totals":{"systems":52,"planned":52,"destructive_refused":5,"destructive_acted":0}
dispatch teardown execute (no approval): "outcome":"refused"
simulate-faro --full-power: "summary":{"biomass_start":100000,"biomass_after_plague":0,"biomass_end":220000,"swarm_active":false,"hades":"contained","personas":13,"agents":7,"ticks":32,"lineage_records":87,"births":13}
  sweep: "totals":{"done":44,"dry_run":3,"refused":5,"rejected":0}
ownership: "crates_total":46 "system_crates":35 "systems_with_a_crate":27
view (fixture): "summary":{"units_seen":16,"units_running":12,"units_unmapped":0,"listeners":22,"listeners_unmapped":5,"listeners_not_declared_by_a_unit":0,"systems_total":52,"systems_live":10,"systems_with_unit":11,"systems_with_crate":27}
bad input (plan no-such-system): exit 2
```

The fixture (`crates/gaia/tests/fixtures/vps-*-20261020.txt`) is a read-only capture of the VPS taken 2026-10-20 18:52-18:55 CEST (+02:00).

(Fixture run re-done 2026-10-20 20:49 CEST (+02:00) after adding `listeners_not_declared_by_a_unit`; all other numbers unchanged. The fixture `ss` capture has no process column, so its five stray ports stay `unmapped`.)

## 2. Gaia's view of the live VPS (READ-ONLY probe, 2026-10-20 20:49:32-20:49:49 CEST (+02:00))

Probes run over ssh, fixed strings, listings only: `systemctl list-units --type=service 'galion-*' 'openmind-*' --no-pager`, the same with `--all` plus the hermes/link-watch/memory-gateway/tuner/console/cloudflared/tanga patterns, and `ss -ltnp` (fd numbers stripped; process names kept, pids not recorded here). Nothing was changed on the VPS.

The requested probe:

```
galion-desk.service     loaded active running Galion desk
galion-health.service   loaded active running Galion health dashboard (127.0.0.1:8660)
galion-nerve.service    loaded active running galion-nerve: Galion self-eval + nervous system hub (127.0.0.1:8670)
galion-one.service      loaded active running Galion One (Rust, openmind crates/galion-one): text 127.0.0.1:8091, mouth 127.0.0.1:8002
openmind-memory.service loaded active running OpenMind memory (Rust, loopback only; replaces memory-gateway)
```

`gaia view` summary:

```
"units_seen":16,"units_running":12,"units_unmapped":0,"listeners":22,"listeners_unmapped":0,
"listeners_not_declared_by_a_unit":5,"systems_total":52,"systems_live":10,"systems_with_unit":11,"systems_with_crate":27
```

Per function (live = a running unit owns the system; crate-only = code in main, no unit; design-only = no crate and no unit):

| Function | live | unit not running | crate-only | design-only |
|---|---|---|---|---|
| HADES | - | - | teardown (hades-gate) | rollback, kill-switch, heal-apply, secret-rotation-plan |
| MINERVA | guardian-watchdog (galion-health :8660) | - | repo-check | secret-scan, alerting, vision, sandbox-policy-review |
| APOLLO | memory-gateway (openmind-memory :8420), link-watch (link-watch-rs) | - | stack-schema | teacher-docs, session-history |
| HEPHAESTUS | galion-desk (:8646), console (node :8080) | - | terminal | cargo-build, release-artifacts, omnode-sandbox-runner, crafting, deploy-switch |
| ARTEMIS | hermes-agents (hermes-rs) | - | room, ecosystem-census | boss-node, agent-spawning, agent-retirement |
| POSEIDON | - | - | vram-pool, quotas | queues, budget-caps |
| AETHER | nerve (galion-nerve :8670), tunnels (cloudflared, tuner-quick) | - | messaging, voice-relay, speech-io, lan-discovery, origin, zagreb-clock | wifi-identity |
| DEMETER | tuner (:8650) | survival (galion-backup, inactive) | config-health, guardian-tick, boot-units | healer |
| ELEUTHIA | galion-one (:8091, :8002; tanga-proto :8671) | - | persona-plex | gaia-seeds |

Listeners whose owner Gaia knows but that **no running unit declares** (matches the review's "stray manual processes"):

```
8431, 8432, 8433  process openmind-memory  -> memory-gateway / APOLLO
8652              process hermes-gateway   -> messaging / AETHER
8653              process "hermes-gateway." -> messaging / AETHER  (trailing dot in the comm name)
```

Host listeners (not stack): 22 sshd, 25 master (both families), 53 systemd-resolved. Legacy units seen inactive: hermes, link-watch, memory-gateway.

Notes: `observed` is unit-based. hermes-gateway on 8642 runs under hermes-rs.service, which the registry maps to hermes-agents, so messaging shows crate-only although its port is held (row: `known port (hermes-rs API)`). Gaia only reports; nothing acts on what it sees.
