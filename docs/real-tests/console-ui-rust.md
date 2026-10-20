<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: swarm/console-ui-rust (Rust-rendered console pages)

Ports: 19370-19379 (this task). 19371/19372 were held by another agent's `galion-voice-serve` during the run, so
the front used **19375**; the read-only SSH forward used 19379 (VPS 127.0.0.1:8080, the live console API) and 19378
(VPS 127.0.0.1:8091, desk pulse). Only GET requests reached the VPS; no POST, no `/api/v1/markers/stream`, nothing
written on the VPS. All POST paths (pairing, `/api/v1/command`) were tested against a fake upstream only
(`crates/console-static/tests/ui.rs`).

## Command (Rust only; the shell script `scripts/real/console-ui-rust.sh` was removed)

```
GALION_REAL=1 CONSOLE_UI_VPS=root@<vps> CONSOLE_UI_SHOTS=/workspace/swarm/artifacts/console-ui/screens \
  cargo test --release -p console-ui --test real_console_ui -- --ignored --nocapture
```

`crates/console-ui/tests/real_console_ui.rs` (`#[ignore]` + `GALION_REAL=1`) builds the release front, opens the
read-only `ssh -N` forward itself (19379 -> VPS 8080, 19378 -> VPS 8091) unless one already answers, starts
`--ui rust` on 19375, GETs every Rust page and every unported room, re-reads the same live payloads directly and
asserts each page shows exactly them, checks the gate on three public hosts, then takes the screenshots. It never
POSTs, never opens `/api/v1/markers/stream` and no longer calls `/api/v1/ping` (the shell script used it as a
liveness probe; that handler probes the public hosts, so `/api/v1/health` is used instead).

## Output (2026-10-20 21:46 +02:00; screenshot lines omitted)

```
real: start 2026-10-20T21:46:00+02:00
real: binary bytes 601480
real: forward 127.0.0.1:19379 -> VPS 8080, 127.0.0.1:19378 -> VPS 8091 (ssh -N)
galion-console-static: listening 127.0.0.1:19375 ui rust root none files 0 skipped 0 pages 1 upstream 127.0.0.1:19379 prefixes ["/api/", "/_serverFn/"]
real: GET /status -> 200 3484 B 0.729 s, <script> tags: 0
real: GET /services -> 200 18910 B 0.827 s, <script> tags: 0
real: GET /agents -> 200 14928 B 0.871 s, <script> tags: 0
real: GET /domains -> 200 15380 B 0.831 s, <script> tags: 0
real: GET /repos -> 200 19715 B 0.830 s, <script> tags: 0
real: GET /docs -> 200 20190 B 0.830 s, <script> tags: 0
real: GET /cloudflare -> 200 18982 B 0.835 s, <script> tags: 0
real: GET /pack -> 200 16543 B 0.830 s, <script> tags: 0
real: GET /chill -> 200 15433 B 0.828 s, <script> tags: 0
real: GET /jupyter -> 200 13890 B 0.873 s, <script> tags: 0
real: GET /data -> 200 13751 B 0.871 s, <script> tags: 0
real: GET /soon -> 200 743 B 0.000 s, <script> tags: 0
real: GET /_ui/console.731ba1c49b477082.css -> 200 12796 B
real: 12 Rust pages, 17 rooms answer "Not in Rust yet": /console /instances /hermes /context /memory /bank /cache /mesh /arena /analytics /gateway /security /guardian /keys /node /markers /session
real: /status 9 subsystems online · 4 held, 15 hosts, healthy=true
real: /services up 12 down 4 pulse-only 3 (pulse rows 6)
real: /agents version 3.8.3-board pod ip50
real: /domains 15 rows = /api/v1/domains count 15
real: /repos 20 repositories indexed: 12 public, 6 private visible
real: /docs 37 routes (live 25, fridge 12; API says live 25 held 12); ?q=voice -> 0+4
real: /cloudflare 5 steps, 4 zones, 16 records, cname matches
real: /pack compose 1655 B + Caddyfile 451 B shown verbatim
real: /chill 7 asleep = 4 heavy + 3 fridge (services payload says chill 7)
real: /jupyter https://ip50-8888.proxy.runpod.net/lab
real: Host console.galion.app /agents /repos /pack -> "This phone", no page data
real: Host galion.app /agents /repos /pack -> "Soon online.", no page data
real: Host developer.galion.app /agents /repos /pack -> "Discussions", no page data
real: front RSS 2988 kB
real: end 2026-10-20T21:47:14+02:00
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 73.48s
```

Same numbers as the 21:20 shell run for the first three pages (9 online · 4 held, 15 hosts, 12 up / 4 down / 3
pulse-only, 3.8.3-board / ip50). A second run at 21:48 +02:00 (no screenshots, 36.95 s) matched line for line.

Every row is an assertion, not a print: `/domains` each of the 15 rows is `host` + `brand · service name` (role when
the service id is unknown) from `/api/v1/domains` + `/services`; `/repos` every name and role of the 20 items and
the counts from `/api/v1/repos`; `/docs` every one of the 37 paths and the live/fridge split, plus the `?q=voice`
filter computed independently; `/cloudflare` every step, zone, record host and the tunnel CNAME; `/pack` the compose
file and Caddyfile byte for byte (HTML-escaped); `/chill` the 4 heavy services + 3 fridge extras from
`/api/v1/ledger` (= the `chill: 7` of `/api/v1/services`); `/jupyter` the link from `target.podId`.

Page latency (~0.8 s) is the SSH forward's round trips (2-4 upstream GETs per page, in parallel); on the VPS the
upstream is loopback.

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

### Second pass: the catalog rooms against the live console (2026-10-20 21:46 +02:00)

| room | live console 8080 (TSX) | Rust |
|---|---|---|
| `/repos` | says "Fourteen public repositories indexed" (text hard-coded in `repos.tsx`) | "20 repositories indexed: 12 public, 6 private visible" from `/api/v1/repos` (12 public + 6 private + 2 forks) |
| `/domains` | 15 rows from compiled-in `DOMAINS` | 15 rows from `/api/v1/domains`, same text |
| `/docs` | 37 routes, keystroke filter | 37 routes (25 live, 12 fridge), `GET ?q=` filter |
| `/cloudflare`, `/pack` | Copy buttons (clipboard) | same text in selectable `<pre>`; records as text under a `<details>` |
| `/chill` | 4 heavy + 3 extras from compiled-in constants | the same 7 from `/api/v1/ledger` |
| `/jupyter` | link from compiled-in `TARGET.podId` | link from `/api/v1/health` `target.podId` (validated `[A-Za-z0-9-]`) |
| unported rooms | SPA | "Not in Rust yet" page in the console shell, listing the 12 Rust pages |

The live rooms themselves cannot be screenshotted unpaired (client PhoneGate, see below), so the comparison is
against the TSX source of the live tree (copied read-only 21:25 +02:00, still 112 uncommitted entries) and the live
API it renders from.

## Screenshots (headless Chrome 412x915, on the box)

`/workspace/swarm/artifacts/console-ui/screens/`:

- before, live 8080 through the forward: `before-live-status.png` (the "Origins are dark." board),
  `before-live-services.png`, `before-live-agents.png` (both stuck on the client PhoneGate "Obsidian / Loading"
  screen: the live client gate asks even loopback users to pair, so these rooms cannot be screenshotted unpaired);
- before, 2169d1c asset set through console-static: `before-2169d1c-{status,services,agents}.png` (older shell);
- after, Rust (21:46 +02:00 run): `after-{status,services,agents,domains,repos,docs,cloudflare,pack,chill,jupyter,data,soon}-{dark,light}.png`
  (24), `after-not-ported-hermes.png`, `after-pair-console-host.png` (console host without the cookie, on `/repos`).

## Gates on this branch (2026-10-20 21:50 +02:00)

- `cargo fmt --all -- --check`: clean
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo test --workspace`: 559 passed, 0 failed, 1 ignored (the real-life test). console-ui 22 unit (+5);
  console-static `tests/ui.rs` 10 end-to-end (+3: every Rust page GET-only and script-free with the 17 others
  "Not in Rust yet", catalog room content incl. escaping and the `?q=` filter, gate on the read rooms)
- `scripts/no-python.sh`: ok; the branch adds no `.sh` or `.py` vs main (both `scripts/real/*.sh` were ported to
  Rust tests and deleted)
- repo-check: 767 files, 0 errors, 43 warnings (all pre-existing). The first run flagged the VPS address in the two
  new Rust tests; it is now taken from `CONSOLE_UI_VPS` / `CONSOLE_STATIC_VPS`
- builds: x86_64-unknown-linux-musl release 701,216 B, aarch64-unknown-linux-musl release 616,128 B (rust-lld);
  `cargo check` armv7-unknown-linux-musleabihf and aarch64-linux-android clean; no target-cpu, SIMD or asm
