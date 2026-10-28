<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks for slot 392 publish-path-refresh (2026-10-28 15:36-15:40 CET, +01:00)

All read-only. No secrets read or printed. No VPS writes, no service change, no push to main/main-staging.

## GitHub API (read-only, 15:36 CET)
- `main` head 13a296ce15b5bbd66adc26f6bfae787b2c1b5190 (unchanged).
- PR #12 `swarm/main-staging` -> `main`: state open, merged false, mergeable true, mergeable_state clean,
  head 38a07700018906fc21de7ebce1500bdcc4762671 (unchanged since 05:49).
- `GET commits/<sha>`: f68def1d, 4828997d, 7fc44a00, 99cc25bd, 65fa0271, 720548e0, 8f40f18e ->
  422 "No commit found" (local only). 9dda6146 found (carried by 388). Positive control 83e573e2 found.

## VPS (ssh, loopback GETs and `systemctl` reads only; read at 15:37:05 CET)
- is-active: galion-nerve, gaia-cortex, console, console-rust-primary, hermes-rs, galion-one active; hermes-gateway inactive.
- ExecStart binaries: `openmind-nerve-9d1a4f3`, `gaia-cortex-73583ab-llm` (unchanged).
- HTTP codes: 8670 /api/v1/state 200, 8674 /health 200, 8080 / 302, 8080 /api/v1/admin 404,
  18080 /api/v1/admin 404, 8080 /api/v1/bank 404, 8002 /health 503, 8642 /health 200.
- Cortex /health: ok true, mode live, nerve true, incidents_open 2, escalations_open 11, handoffs 7, llm_enabled true.
- Nerve /state: cycle 97, score 64, mode live; alerts external "none configured"; open incidents 2:
  inc-865945dd mouth stuck loading 21.2 h; inc-06ffb32f "16 public hostname(s) to 8 closed origin port(s)
  :3011 :3333 :4001 :4003 :4020 :4060 :8000 :18800: 390.9 failed requests/min" (rate inflated, F-380-1).
- `stat` metadata only: `/opt/galion/galion-light/publish/obsidian/link.env` 666 root:root.

## Local merge rehearsal (`git merge-tree --write-tree`, no build, scratch refs only)
- 83e573e2 (373) + d50478db (374): exit 0, **0 conflicts**, tree 76ece3485b0d0cffad77fab4ccc4b39a03cf528f;
  merge-base 3f015986 (= main-staging 38a07700 tree snapshot).
- that result + 9ebbed3a (386, 7 commits): exit 1, conflicts `.cargo/config.toml` (comment block next to the same
  `linker = "rust-lld"` line, already in 373) and `crates/openmind-nerve/tests/real_cross_target.rs`.
- that result + 403f7179 (390, 9 commits): exit 1, conflicts `crates/console-static/tests/real_landing.rs`,
  `crates/openmind-nerve/tests/real_cross_target.rs` (test files only).
- `git show 83e573e2:.cargo/config.toml` has `[target.x86_64-unknown-linux-musl] linker = "rust-lld"`.

No builds and no cargo tests: doc-only task (row 392 "Doc only, no builds").
