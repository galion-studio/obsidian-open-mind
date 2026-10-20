<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: swarm/console-ui-rust (Rust-rendered console pages)

Ports: 19370-19379 (this task). 19371/19372 were held by another agent's `galion-voice-serve` during the run, so
the front used **19375**; the read-only SSH forward used 19379 (VPS 127.0.0.1:8080, the live console API) and 19378
(VPS 127.0.0.1:8091, desk pulse). Only GET requests reached the VPS; no POST, no `/api/v1/markers/stream`, nothing
written on the VPS. All POST paths (pairing, `/api/v1/command`) were tested against a fake upstream only
(`crates/console-static/tests/ui.rs`).

## Command

```
SHOTS=/workspace/swarm/artifacts/console-ui/screens scripts/real/console-ui-rust.sh
```

## Output (2026-10-20 21:20 +02:00)

```
real: start 2026-10-20T21:20:00+02:00
real: binary bytes 564792
galion-console-static: listening 127.0.0.1:19375 ui rust root none files 0 skipped 0 pages 1 upstream 127.0.0.1:19379 prefixes ["/api/", "/_serverFn/"]
real: GET /status -> 200 3484 0.774330 s, <script> tags: 0
real: GET /services -> 200 18910 0.821092 s, <script> tags: 0
real: GET /agents -> 200 14928 0.850627 s, <script> tags: 0
real: GET /_ui/console.e22175fdf7cef707.css -> 200 9299 B
real: Host console.galion.app /agents -> 200 961 B
real: Host galion.app /agents -> 200 668 B
real: Host developer.galion.app /agents -> 200 943 B
real: front RSS 2748 kB
real: /status 3484 B, 9 subsystems online · 4 held, 15 hosts, healthy=true
real: /services 18910 B, up 12 down 4 extras 3 pulse 6
real: /agents 14928 B, version 3.8.3-board
real: gate console.galion.app -> pairing page; galion.app -> Soon
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.03s
real: end 2026-10-20T21:21:04+02:00 fail=0
```

`tests/real_console_ui.rs` re-reads the same live API (`/api/v1/health`, `/services`, `/domains`, desk
`/api/v1/awake`) and asserts each page shows exactly that: the status counts (9 online, 4 held), every one of the
15 hosts, every service name in the right Up/Down group (12 up, 4 down, 3 pulse-only cold rows), the origin version
`3.8.3-board` and pod `ip50`; the public console host without a cookie gets the pairing page with none of that data.

Page latency (~0.8 s) is the SSH forward's round trips (3-4 upstream GETs per page, run in parallel); on the VPS
the upstream is loopback.

## Live comparison (same minute, same data)

| | live console 8080 (Vite dev, React) | Rust front `--ui rust` |
|---|---|---|
| process RSS | 1,294,044 kB node + 32,132 kB wrapper (VPS `ps`, 21:17 +02:00) | 2,748 kB |
| `/status` headline | "Origins are dark." (fresh browser) | "Light stack is up." (from `/api/v1/health` status=healthy) |
| `/status` counts | 9 online · 4 held | 9 online · 4 held |
| `/agents` Origin / Persist / Pod | 3.8.3-BOARD / "/WORKSPACE" (hard-coded) / "—" | 3.8.3-BOARD / "not in /api/v1/health" / IP50 |
| `/services` Start/Stop | buttons flip browser storage only | not rendered; page says nothing is started here |
| JS shipped | React SPA (871,756 B asset set at 2169d1c) + dev runtime | 0 bytes; CSP `default-src 'none'` |

The fail-closed path was also seen for real: when the SSH forward dropped at 21:15 +02:00, the console host got
**503 "Console API is not answering"** (no page data) and loopback pages rendered with the origin shown as down.

## Screenshots (headless Chrome 412x915, on the box)

`/workspace/swarm/artifacts/console-ui/screens/`:

- before, live 8080 through the forward: `before-live-status.png` (the "Origins are dark." board),
  `before-live-services.png`, `before-live-agents.png` (both stuck on the client PhoneGate "Obsidian / Loading"
  screen: the live client gate asks even loopback users to pair, so these rooms cannot be screenshotted unpaired);
- before, 2169d1c asset set through console-static: `before-2169d1c-{status,services,agents}.png` (older shell);
- after, Rust: `after-{status,services,agents}-{dark,light}.png`, `after-pair-console-host.png`.

## Gates on this branch (2026-10-20 21:24 +02:00)

- `cargo fmt --all -- --check`: clean
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo test --workspace`: 552 passed, 0 failed (console-ui 24 unit + console-static 31 incl. 7 new end-to-end `tests/ui.rs`)
- `scripts/no-python.sh`: ok
- repo-check: 768 files, 0 errors, 43 warnings (all pre-existing; base 6bd3580 has the same 43; none in files of this branch)
- builds: x86_64-unknown-linux-musl release 668,448 B, aarch64-unknown-linux-musl release 585,504 B (rust-lld);
  `cargo check` armv7-unknown-linux-musleabihf and aarch64-linux-android clean; no target-cpu, SIMD or asm
