<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: post-publish-test-batch (wave-4 slot 415), 2026-10-28 CET (+01:00)

Tree: `swarm/post-publish-test-batch` 487c5797 (373 83e573e2 + 374 + 375/376/383/386/390).
Slot ports 22850-22859. Real binaries, release builds, loopback only; every started process
was stopped by the tests (ports 22850-22859 free afterwards, `ss -ltn`, 18:21).

## Box, GALION_REAL=1 (release)

| time | test (conflict-touched file) | env | result |
|---|---|---|---|
| 18:15-18:16 | `console-static --test real_landing` (debug) | `LANDING_REAL_PORT_BASE=22850` | 1/1 ok: `/console` 200 31,678 B, 9 cards, 2 svg, 0 `<script>`; base css 20,324 B + overview css 6,040 B; `/` 302 -> /console; status/services/agents/docs/soon/hermes 200; light-theme cookie honoured |
| 18:16-18:17 | same, `--release` | same | 1/1 ok, same numbers |
| 18:17-18:18 | `hermes-gateway --test real_enroll` (Cargo.toml/lock union) | `HERMES_ENROLL_REAL_PORT_BASE=22850` | 71 pass / 1 fail: "rejected B: claim refused: want 2 true, got 2 false" = **F-383-5, already recorded by slot 383 on its own branch (71/72)**; not caused by this merge. Ports 22850-22853 free after teardown |
| 18:19-18:21 | `openmind-nerve --test real_cross_target` (3 merges resolved here), default `NERVE_XT_PIN=budget` | `NERVE_XT_PORT_BASE=22855` | **8/8 ok, 98.5 s**: x86_64 musl 2,017,768 B (WARN above 90 %, headroom 79,384 B to 2,097,152 B = size-budget.tsv row), aarch64 1,666,696 B static, live recipe == solo (sha ee9f33e9...), +559,008 B vs live 9d1a4f3; smoke on :22855/:22856 (qemu aarch64)/:22857 first cycle 0.10/0.21/0.10 s, /health wire order equal on all 3; android aarch64 + armv7 `cargo check` ok; `repo_config_pins_rust_lld` ok on 373's config (x86_64, aarch64, armv7 = rust-lld) |

The nerve size matches slot 373's recorded 2,017,768 B: the test batch does not change the
release binary.

## VPS, read-only (ssh, GET only, nothing written)

| time | probe | result |
|---|---|---|
| 18:19:16 | `curl http://127.0.0.1:8670/state` (live nerve) | mode live, cycle 3 (started 17:40:13, deploy t334u), self_eval score 68 D, 15 components, 1 failing (tuner-quick), 3 incidents |
| 18:19 | `systemctl is-active galion-nerve gaia-cortex` | active / active |
| 18:19:49 | `curl http://127.0.0.1:8674/health` (live cortex) | ok true, mode live, nerve true, llm_enabled true, incidents_open 2, escalations_open 0 |

Cortex is live (deploy t334u ~17:40); observed through it and through the nerve.
