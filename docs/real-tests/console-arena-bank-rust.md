<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-arena-bank-rust (wave 4 slot 299) — real tests

Branch `swarm/console-arena-bank-rust` off `swarm/console-v2-sprite-health` 58726e7c.
Box only, ports 21690-21699. No live console deploy, no VPS writes.

## Change
- `console_ui::SPA_FIRST` is now empty: `/arena` and `/bank` are Rust-first even when an
  SPA fallback (`--root` + shell) is configured. Every ported route is Rust-first.
- `/arena` takes `Result<&Value, Miss>` and says why a board is missing: 404 "no /api/arena
  route yet", 401/403 "sign in", other HTTP status, or no answer. Empty board says so instead
  of an empty list. Breed/Stop/Promote stay held (paid run, Slavik's spend call); the stale
  "stay on the TypeScript console" note is gone (the Node console is retired).
- `/bank` note no longer points at the TypeScript console: balances show once a Rust bank
  reader serves `/api/v1/bank` (owner decision).
- No CSS change (R1 budget untouched: base <=20 KiB + overview <=8 KiB asserted in css.rs tests).
- No new dependencies.

## Live read-only baseline (VPS, 2026-10-27 20:41 CET +01:00)
- `console.service` = `galion-console-serve-2d846fb8 --ui rust --local-api` on :8080 (primary :18080).
- `GET 127.0.0.1:18080/api/arena` -> 404, `/api/v1/bank` -> 404, `/api/v1/health` -> 200.
- `GET :8080/arena` -> 200 14148 B (says "did not answer"), `/bank` -> 200 14957 B ("route yet").
- 20:44 CET: nerve `:8670/state` 200 38129 B, cortex `:8674` 200, console.service active.

## Box real tests (release binary), 2026-10-27 20:42 CET +01:00
`GALION_REAL=1 CONSOLE_ARENA_LIVE=1 CONSOLE_ARENA_VPS=<vps> cargo test --release -p console-static --test real_arena_bank -- --nocapture --test-threads=1`

```
real_bare_api_like_live_404:   bare GET /arena -> 200 14165 bytes 1 ms
                               bare GET /bank  -> 200 14949 bytes 0 ms
                               bare 40 renders, worst 0 ms
real_full_api_rust_first_with_spa: full GET /arena -> 200 14494 bytes 1 ms
                               full GET /bank  -> 200 14912 bytes 0 ms
                               upstream calls: 3 (GET only; 2 POSTs -> 405, never reached the API)
real_live_vps_upstream_readonly: live upstream: /api/arena -> 404, /api/v1/bank -> 404
                               live GET /arena -> 200 14165 bytes 746 ms
                               live GET /bank  -> 200 14949 bytes 613 ms
test result: ok. 3 passed; 0 failed
```
Each case serves a fake SPA (`_shell.html` with a `<script>`, routes incl. `/arena`, `/bank`):
the rooms never return the shell, carry no `<script>`/`<form>`, and `/spa-only` still gets the shell.
The live leg is an `ssh -L 21699 -> VPS 127.0.0.1:18080` tunnel, GETs only.

Negative control: with `SPA_FIRST = ["/arena", "/bank"]` restored, `real_full_api_rust_first_with_spa`
and `real_bare_api_like_live_404` FAIL ("SPA shell served, not Rust"), so the test detects the old behaviour.

## Gates
- `cargo fmt --check` ok; `cargo clippy -p console-ui -p console-static --all-targets -D warnings` ok.
- `cargo test -p console-ui -p console-static`: console-ui 106/0, console-static lib 24/0, ui 12,
  ui_port1 6, ui_port2 8, ui_port3 7, ui_port4 7, ui_port5 9, serve 6 — all pass; live-only tests ignored.
- GALION_REAL release: real_sprite_health 1/0, real_landing 1/0.
- Builds: x86_64-musl release 947072 B; aarch64-musl release 802112 B (box linked with rust-lld,
  the restored box has no aarch64 cc); `cargo check` aarch64-linux-android, armv7-linux-androideabi,
  armv7-unknown-linux-musleabihf ok.
- `scripts/no-python.sh` ok. Nothing else depends on console-ui/console-static.
