<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# How the console's browser UI gets built (decision record, swarm/console-ui-rust)

- Written 2026-10-20 21:20 +02:00 (Europe/Zagreb, CEST). Base: swarm/console-static 6bd3580.
- Inputs: live console-ide tree on the VPS read 2026-10-20 21:05 +02:00 (read-only `tar` to stdout of `src/` and
  `server/`, `.env*` and `*.bak*` excluded, filename-only secret scan = 0 hits; nothing written on the VPS), the
  2169d1c archive and asset set in `/workspace/swarm/artifacts/console-static/`, live API answers through a GET-only
  SSH forward.
- Status: recommendation below; the first three pages are built and tested on this branch. **Nothing deployed.**

## Recommendation

**Option (c), server-rendered: the Rust front renders the pages as HTML from the console API, with a stylesheet
generated from Rust and no JavaScript.** Port page by page in priority order. It is the only option that meets
"Rust only, no exceptions" for 27 of the 29 console rooms *without* needing the open ruling on browser JavaScript,
it adds zero dependencies and zero new targets (it is the same std-only binary that already builds for x86_64/aarch64
musl and checks on armv7 and android), and it is already working for `/status`, `/services` and `/agents` against
live data.

Two rooms cannot be fully JavaScript-free, because browser APIs they use exist only in JavaScript:
**Hermes Research** (`/hermes`: mic capture via `getUserMedia`/`MediaRecorder`, streamed answers) and the
**Desk** (`/console`, which embeds the same chat). For those, Slavik's JS ruling decides between a small hand-written
script island and a reduced no-JS version (details under "What Slavik must decide").

## What the UI really is (inventory of the live tree, not the 2169d1c build)

| | 2169d1c (the built asset set) | live working tree 2026-10-20 |
|---|---|---|
| uncommitted entries (`git status --porcelain`) | - | **112** (43 modified, 69 untracked; the "~60" in console-static.md undercounts) |
| page routes | 16 | **31** (29 console rooms + `/status` + `/soon`) |
| phone gate | none | `src/start.ts` (server middleware) + `components/phone-gate.tsx` (client) |
| page code | | 3,685 lines across the 31 route files |
| shared UI | | `hermes-panel.tsx` **2,758 lines** (the one big component), `ide-shell.tsx` 258, `lib/hermes.ts` 786, `styles.css` 401 |
| Tailwind | | **353 distinct utility classes** in 1,139 `className` attributes |
| dead code | | `lib/multiplayer/p2p.ts` (570 lines WebRTC) is imported by no page |

Per page (live tree; API = what the browser calls; "poll" = `setInterval` refresh; serverFn = TanStack RPC that runs in
Node and reads local files or loopback ports):

| page | lines | data | interactivity | no-JS port |
|---|---|---|---|---|
| status | 68 | catalog via store (no fetch) | none | **done** |
| services | 188 | serverFn -> `127.0.0.1:8091/api/v1/awake` | Start/Stop (fake, see below) | **done** |
| agents | 96 | `GET /api/v1/health`, `POST /api/v1/command` | 3 buttons | **done** (form POST) |
| soon, data, jupyter, domains, repos, chill, pack, docs, node, cloudflare, md.$name, index | 7-92 | static catalog / one GET | copy buttons, tabs | easy (copy-to-clipboard needs JS; show the text selectable instead) |
| gateway | 93 | arbitrary GET to `/api/v1/*` | request builder | form GET, server performs the call |
| markers | 86 | `/api/v1/markers` (+ the stream elsewhere) | 1 POST | form POST; live tail = meta refresh or EventSource |
| keys, guardian, arena, session | 78-118 | own `/api/*` | 1-2 POSTs | forms |
| mesh, instances, analytics, security | 53-394 | own `/api/*` | **poll 5-8 s** | `<meta http-equiv=refresh>` or a 10-line fetch script |
| bank, cache, context, memory | 124-215 | **serverFn** (reads `/opt/galion/...` files, desk-foundation) + poll 4 s | | needs the data as a plain `/api/v1/*` endpoint first (TanStack serverFn RPC is not a stable wire format) |
| console (Desk) | 607 | serverFn x4, `/api/hermes/health`, `/api/v1/analytics`, poll 4-5 s, chat | chat form | partial without JS |
| hermes (Research) | 10 + 2,758 | `/api/hermes/*` (chat SSE via fetch reader, runs, loops, history, modes, tui, guard, settings, transcribe), localStorage threads | streaming chat, mic recording, mode switches | **needs JS** for mic and token streaming |

Live streams: `marker-rail.tsx` uses `EventSource('/api/v1/markers/stream')` but is only mounted by the old
`shell.tsx`, which the live `ide-shell.tsx` replaced, so no live page opens it today. Hermes chat streams over
`fetch()` + `getReader()` on a POST, which `EventSource` (GET-only) cannot replace one-to-one.

## Findings that change the picture (measured, not assumed)

1. **The status board is a client-side simulation.** `/status` reads a zustand store in `localStorage`, seeded from
   the catalog; nothing probes anything. Measured: a fresh headless browser gets live `/status` saying
   **"Origins are dark."** while listing 9 hosts as ONLINE (screenshot `before-live-status.png`, same on the 2169d1c
   build). Likely cause: `FleetHydrate` subscribes to `onFinishHydration` after the synchronous `rehydrate()` has
   already finished, so `boot()` never runs. The Rust page shows
   the headline from the real `/api/v1/health` ("Light stack is up.") and marks a host offline when the desk pulse
   reports its service down.
2. **Start/Stop on `/services` never touched a service.** `toggle()` only flips the browser-stored state. The Rust page
   drops the buttons and says so on the page.
3. **`/agents` "Pod" is always "—" and "Persist" is a hard-coded "/workspace".** The TSX reads `health.pod` and
   `health.persist`; `/api/v1/health` has neither (pod is at `target.pod` = `ip50`). The Rust page shows `ip50` and
   "not in /api/v1/health" (held tone) instead of a green made-up value.
4. **Two gates disagree.** The server gate (`start.ts`) lets any request whose *Host header* is `127.*`/`localhost`
   through; the client `PhoneGate` still asks loopback users to pair (it checks the cookie only). The Rust gate is one
   server-side decision: loopback Host **and** loopback TCP peer, otherwise the admin cookie, verified by the upstream's
   own `GET /api/v1/admin` (so this process never reads `/root/galion-secrets/admin-device.json`). Unpaired browsers get
   the pairing page and **no page data at all**; today they can still download the whole SPA bundle.
5. **The Vite dev server is still the production console**: 1.23 GiB RSS (details in the status report), serving
   `/package.json`, `/vite.config.ts` and `src/**` source. The Rust front with the Rust pages runs at **2.7 MB RSS**.

## The options against the rules

| | (a) serve pre-built assets | (b) Rust build tooling for the TSX | (c1) WASM (Leptos/Yew/Dioxus) | **(c2) server-rendered Rust (chosen)** |
|---|---|---|---|---|
| Rust-only rule | no: every UI change needs Node/npm/Vite (425 npm packages) somewhere; the live tree (112 changes) has **never been built**, so even "today's" UI needs one more JS build | partial: build in Rust (oxc/swc + rolldown, MIT), but the browser still runs React; Tailwind v4's compiler is JS (only the "oxide" scanner is Rust) so 353 classes need a re-implementation | Rust in the browser, but wasm-bindgen ships a generated JS shim: still "JS in the browser", so it also waits on the ruling | yes for 27/29 rooms with zero JS; 2 rooms wait on the ruling |
| deps / supply chain | 425 npm packages (licence table exists) | oxc + rolldown + npm resolver: hundreds of crates | wasm-bindgen, web-sys, a framework, trunk/cargo-leptos: 100-300 crates | **0 new crates** (std only) |
| portability (x86_64/aarch64 musl, armv7, android; no SIMD flags) | n/a (assets) | build tool must run on the box only; fine | adds the wasm32 target to the toolchain; server side unchanged | **same binary as today**; built x86_64 + aarch64 musl, checked armv7 + android |
| fidelity to today's UI | exact for 2169d1c, stale vs live | exact if the toolchain matches | rewrite | rewrite; same tokens, icons, copy and layout (screenshots below) |
| Oct 31 risk | low effort, but blocked by the rule | **high**: multi-day bespoke toolchain, mismatched output, then maintained forever | high: 4k+ lines of React rewritten in a new framework plus a new build | **medium**: 3 rooms took ~1.5 h including all infrastructure; most rooms are 23-240 lines with 0-2 calls |
| security | SPA bundle public to unpaired users | same | same | gate before any data; strict CSP `default-src 'none'` (no script can run at all), forms CSRF-checked |

## What was built (swarm/console-ui-rust)

- `crates/console-ui` (new, std only, `#![forbid(unsafe_code)]`): `json` (strict RFC 8259 reader/writer), `client`
  (loopback HTTP/1.1, Content-Length/chunked/close, 4 s budget, 2 MiB cap), `gate` (port of `start.ts` + `host.ts`),
  `css` (tokens from `styles.css` `@theme` as Rust constants + ~110 hand-written rules, 9.2 KB, served as
  `/_ui/console.<fnv64>.css`, immutable), `icons` (the console's own SVG path data), `rooms` (room list + dock),
  `layout` (shell: header, `<details>` menu, dock; theme via `/_ui/theme` cookie), `pages/{status,services,agents,gate,not_ported}`.
- `galion-console-static --ui rust --upstream 127.0.0.1:8088 [--pulse 127.0.0.1:8091] [--root DIR]` `[--console-host IP]...`
  (the server IPs that `host.ts` hard-coded are flags now, so no public IP is in the code): Rust pages for the
  ported routes; with `--root` the unported routes fall back to a pre-built SPA (only if Slavik allows (a) as an
  interim), without it they get a "Not in Rust yet" page. `/api/*` and `/_serverFn/*` are still proxied as before.
- No JavaScript anywhere: 0 `<script>` tags; CSP forbids scripts. Pairing works without JS: `POST /_ui/pair` relays
  `{"code":...}` to `/api/v1/admin` and passes on **only** the `galion_admin` cookie.

## What Slavik must decide

1. **JS ruling for two rooms** (Hermes Research, Desk chat). Pick one:
   - (i) "no exceptions" includes browser JS: ship a no-JS Research room (form POST, the server waits for the full
     answer or uses a server-side relay; **no mic**, no token streaming, history server-side instead of localStorage);
   - (ii) allow one hand-written, dependency-free script island per room (est. 150-300 lines, no npm, no build step,
     served by Rust, CSP `script-src` with a hash): mic recording + streamed tokens keep working;
   - (iii) keep Research on the old SPA until after Oct 31 (needs (a) for that room only).
   An inline `EventSource` for live tails (markers, analytics) would be ~10 lines under (ii); under (i) those pages
   use `<meta http-equiv="refresh">` (already supported by the page frame approach).
2. **Freeze or redirect console-ide.** The live tree moved from ~60 to 112 uncommitted entries; porting a moving
   target never converges. Proposal: console-ide is frozen at the current working tree (committed to a rev for the
   record), and new console features land in `crates/console-ui`.
3. **serverFn data for bank/cache/context/memory/desk**: these rooms read files and loopback ports inside Node via
   TanStack RPC. Allow porting those reads into Rust API endpoints first (they are read-only file/port reads), or
   expose them as plain `/api/v1/*` on the Node side for phase 1.
4. **Behaviour changes in the ported rooms** (findings 1-3): truthful status headline, Start/Stop removed, Pod/Persist
   fixed. Confirm, or ask for literal parity.
5. **Interim**: may unported rooms fall back to the 2169d1c asset set (option (a), stale, built before the rule) until
   they are ported, or show "Not in Rust yet"?

## Plan to Oct 31 (if (c2) is accepted)

| when (+02:00 until Oct 25, then +01:00) | rooms | notes |
|---|---|---|
| Oct 21 | domains, repos, docs, node, cloudflare, chill, pack, data, jupyter, soon, md | static catalog / one GET each |
| Oct 22 | gateway, markers, keys, session, guardian, arena | forms; POST targets CSRF-checked |
| Oct 23 | mesh, instances, analytics, security | refresh strategy per the JS ruling |
| Oct 24-25 | bank, cache, context, memory | after decision 3 (data endpoints) |
| Oct 26-28 | Desk, Hermes Research | per decision 1 |
| Oct 29-30 | full side-by-side screenshots of all rooms, cutover rehearsal on the box | cutover itself is Slavik's (console-static.md) |
