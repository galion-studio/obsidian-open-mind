<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life measurements: console-v2-audit (wave 4 slot 243)

Branch `swarm/console-v2-audit`, based on `swarm/console-port-merge` 508401dd (all 31 console routes in Rust).
Box, Europe/Zagreb (CEST +02:00), 2026-10-22 01:07-01:13 +02:00. Slot ports 21130-21139 (21130-21134 used).
Docs and measurements only: no code changed, no new dependency.

VPS reached only through one `ssh -N` forward (alias `slavik-vps`): box 21131 -> VPS 127.0.0.1:8080 (console),
21132 -> 8646 (galion-desk), 21133 -> 8670 (galion-nerve). **GET only; nothing was started, written, restarted or
changed on the VPS.** One read-only `ss -ltnp` / `ps` / `free` over ssh to identify the :8080 process. No sandbox run.

## Setup
```text
CARGO_TARGET_DIR=/workspace/swarm/target/console-v2-audit nice -n 10 cargo build --release -p console-static -j 4   # 7.98 s warm, 242 MiB peak RSS (cargo)
galion-console-static --ui rust --upstream 127.0.0.1:21131 --desk 127.0.0.1:21132 --bind 127.0.0.1:21130 --quiet   # live data
galion-console-static --ui rust --upstream 127.0.0.1:21139 --desk 127.0.0.1:21138 --bind 127.0.0.1:21134 --quiet   # dead upstream = pure render cost
curl -s -o page -w '%{http_code} %{size_download} %{time_starttransfer} %{time_total}' http://127.0.0.1:2113x<route>
```

## Binary and process
| item | value |
|---|---|
| `galion-console-static` release, x86_64 glibc | 837 992 B (already stripped; `strip` -> 837 984 B) |
| RSS front, live (21130) | 2 964 kB at start -> 4 300 kB (VmHWM) after 90 live GETs |
| RSS front, dead upstream (21134) | 2 940 kB after 150 GETs |
| **live VPS console :8080 today** | `node .../vite/bin/vite.js dev --host 127.0.0.1 --port 8080` (console.service), **RSS 1 299 604 kB**, up 6 d 17 h |
| ratio | the Rust front is ~1/300 of the live console's memory |

The live console on the VPS is the TanStack Start **Vite dev server** (dev client entry, `/src/styles.css` 56 415 B,
Google Fonts links, no production build). The Rust front still proxies `/api/*` and `/_serverFn/*` to it, so the
1.3 GB process cannot be retired until the API it serves is in Rust too (see the V2 plan, phase 3).

## Shared assets (Rust front)
| asset | bytes | gzip -9 | headers |
|---|---|---|---|
| `/_ui/console.00c4d487228c5145.css` | 26 480 | 5 428 | `Cache-Control: public, max-age=31536000, immutable`, ETag |
| `/_ui/favicon.svg` | 353 | - | |
| HTML pages | - | - | `no-store`, strict CSP (`default-src 'none'; style-src 'self'`), nosniff, **no Content-Encoding** (front sends identity even when the client accepts gzip/br) |

## Per route (30 GETs, 3 runs each live, 5 runs each dead-upstream; last live run / best offline run)
`main` = bytes of `<main>...</main>`; `svg` = bytes of inline `<svg>` in the page; `render` = time with a dead
upstream (pure Rust render + refused connect); `live` = time through the ssh forward to the live API. Every page
0 `<script>`, 0 `<style>`.

| route | code | bytes | gzip -9 | main | inline svg | render s | live s |
|---|---|---|---|---|---|---|---|
| `/console` | 200 | 31949 | 7593 | 17989 | 15165 | 0.001658 | 0.825650 |
| `/status` | 200 | 3590 | 1079 | 2462 | 318 | 0.003371 | 0.784659 |
| `/services` | 200 | 19096 | 3313 | 5189 | 9410 | 0.003433 | 0.784597 |
| `/agents` | 200 | 15366 | 2510 | 1353 | 9610 | 0.000355 | 0.828565 |
| `/domains` | 200 | 15819 | 2564 | 1866 | 9532 | 0.002664 | 0.785199 |
| `/repos` | 200 | 20152 | 3260 | 6164 | 9604 | 0.003612 | 0.786343 |
| `/docs` | 200 | 20626 | 3209 | 6740 | 9212 | 0.004311 | 0.785205 |
| `/cloudflare` | 200 | 19424 | 3131 | 5480 | 9243 | 0.000967 | 0.790054 |
| `/pack` | 200 | 16979 | 3333 | 3054 | 9448 | 0.003366 | 0.778970 |
| `/chill` | 200 | 15870 | 2771 | 1967 | 9428 | 0.003336 | 0.780728 |
| `/jupyter` | 200 | 14329 | 2382 | 412 | 9216 | 0.000422 | 0.826933 |
| `/data` | 200 | 14187 | 2339 | 284 | 9207 | 0.000425 | 0.819032 |
| `/soon` | 200 | 849 | 484 | 92 | 0 | 0.000238 | 0.001835 |
| `/analytics` | 200 | 14896 | 2488 | 983 | 9402 | 0.000833 | 0.817641 |
| `/arena` | 200 | 17520 | 3221 | 3609 | 9205 | 0.003813 | 0.841991 |
| `/bank` | 200 | 15278 | 2570 | 1338 | 9433 | 0.003734 | 0.826714 |
| `/cache` | 200 | 27593 | 4503 | 13687 | 9634 | 0.002739 | 0.817754 |
| `/context` | 200 | 21071 | 3971 | 7159 | 9674 | 0.000366 | 0.824955 |
| `/gateway` | 200 | 15774 | 2665 | 1874 | 9414 | 0.000326 | 0.818845 |
| `/guardian` | 200 | 14765 | 2553 | 854 | 9218 | 0.000335 | 0.824796 |
| `/hermes` | 200 | 28077 | 5778 | 14094 | 9486 | 0.010041 | 0.797334 |
| `/instances` | 200 | 17956 | 3131 | 4018 | 9400 | 0.006963 | 0.819510 |
| `/keys` | 200 | 15197 | 2624 | 1274 | 9466 | 0.007340 | 0.821409 |
| `/markers` | 200 | 14875 | 2506 | 921 | 9516 | 0.007382 | 0.826099 |
| `/md/user` | 404 | 886 | 508 | 0 | 0 | 0.000218 | 0.002731 |
| `/memory` | 200 | 15485 | 2570 | 1548 | 9433 | 0.000401 | 0.822650 |
| `/mesh` | 200 | 14505 | 2502 | 606 | 9204 | 0.000469 | 0.849446 |
| `/node` | 200 | 15613 | 2828 | 1719 | 9444 | 0.000326 | 0.819010 |
| `/security` | 200 | 16280 | 2927 | 2336 | 9444 | 0.000491 | 0.826918 |
| `/session` | 200 | 14495 | 2380 | 589 | 9416 | 0.003304 | 0.802220 |

Totals: 30 pages 488 502 B raw, 87 693 B gzip -9 (5.6x). Chrome (header, room sheet, dock) is ~13.9 kB of every
page; ~9.2 kB of it is 42 inline SVGs (33 distinct) repeated on every page. Dock alone 1.77-1.79 kB.

Timing: render is 0.2-10 ms per page (worst `/hermes` 10.0 ms, `/instances`/`/keys`/`/markers` ~7 ms).
Every live page costs 0.78-0.85 s, all of it upstream: one `GET /api/v1/health` through the same forward costs
0.38-0.56 s, so pages make ~1-2 sequential upstream round trips each. On the VPS (loopback) this is a few ms;
over a phone link it multiplies. Upstream fan-out per page should be parallel (or cached) in V2.

Rooms stating live 401 / "did not load" (expected without an operator session; not errors of the front):
`/analytics` (401), `/guardian`, `/keys`, `/session` (401 + did not load), `/security` (3x), `/hermes` (401).

## Dead CSS audit
181 class selectors in the sheet. Every one appears in a rendered page or in a `class=` string in
`crates/console-ui/src` -> **0 dead classes**. 14 appear only in state-dependent markup not reached by today's live
data (`gate msgs p3-bar p3-fresh p3-key p3-kn p3-mkl p3-note p3-pct p3-term p4-md rs say tool`): keep.
Waste is structural, not dead rules:
- `LANDING` block (6 035 B source, 50 `ov-*` classes) is used by **one** page (`/console`) but shipped in the one sheet.
- The light palette is emitted twice (`html[data-theme=light]` 252 B and the `prefers-color-scheme` copy 289 B).
- Sheet sections (source bytes): RULES 11 987, LANDING 6 035, PORT1 1 176, PORT2 2 310, PORT3 2 769, PORT4 974.

Dead JS: none in the Rust front (0 scripts). The JS that is still served is the live Vite dev SPA on :8080, which
the Rust front reaches only for `SPA_FIRST` fallbacks (`/arena`, `/bank`, port2 `/hermes`) when a fallback root is set.

## Upstream API surface used by the Rust pages (from `crates/console-ui/src`)
`/api/v1/{health,services,markers,keys,domains,command,admin,session,session-pack,repos,ledger,catalog,security,
protocol/snapshot,ping,pack,laptop,instances,cloudflare,boot,bank,awake,analytics}`, `/api/v3/guardian/{status,scan,
incidents,heal,<act>}`, `/api/v3/voice`, `/api/hermes/{guard,modes,models,history,health}`, `/api/mesh`, `/api/arena`.
This is the list a Rust console API must serve before :8080 (Vite dev) can be stopped.

## Nerve (VPS 127.0.0.1:8670 `/state`, read-only)
| when (+02:00) | cycle | mode | score | grade | counts | console / galion-desk |
|---|---|---|---|---|---|---|
| 01:07:47 (before) | 16 | live | 56 | D | healthy 11, degrading 1, failing-soon 1, absent 1 | healthy / healthy |
| 01:11:56 (after) | 17 | live | 56 | D | same | healthy / healthy |
No change caused by the probes. (Recent alert in the feed: hermes-rs slow `POST /v1/chat/completions` 22.8 s - not ours.)

## Box checks
- `cargo test -p console-ui -p console-static` (release target dir of this slot): 182 passed, 0 failed, 7 ignored.
- Teardown 01:12 +02:00: both fronts and the ssh forward killed; `ss -ltn` shows nothing on 21130-21139.
- Not run: `real_merge` / `real_port*` (they hard-code slot 169/135-139 ports; this slot measured with the same
  binary and arguments on its own ports instead). `real_console_ui` not run (stale, see plan ruling R3).
