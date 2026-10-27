<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console V2: research and plan (wave 4 slot 243, 2026-10-22 +02:00)

Base: `swarm/console-port-merge` 508401dd (31 of 31 routes in Rust). Numbers come from
`docs/real-tests/console-v2-audit.md` (box + live GET-only over ssh, 2026-10-22 01:07-01:13 +02:00).

## What we have (measured)
- One std-only Rust binary, 838 kB, 3-4 MB RSS, 0 JavaScript, strict CSP, 30 routes + `/md/$name`.
- Render cost 0.2-10 ms per page. Live page time 0.78-0.85 s via ssh = upstream round trips, not Rust.
- Pages 14-32 kB raw (2.3-7.6 kB gzip). ~13.9 kB of every page is chrome; **~9.2 kB of that is 42 inline SVG icons**
  repeated on every page. HTML is sent uncompressed.
- One 26 480 B sheet (5 428 B gzip), immutable-cached. 0 dead classes; 6 kB of it (`ov-*`) serves only `/console`;
  the light palette is emitted twice.
- **The live VPS console (:8080) is a Vite dev server using 1.3 GB RSS** (TanStack Start dev client, 56 kB dev CSS,
  Google Fonts). The Rust front still proxies `/api/*` and `/_serverFn/*` to it, and the header dot of every page
  calls `/api/v1/health` there.

## Goal of V2
Same pages, same zero-JS/CSP stance, less per page and nothing heavy behind it: **~5 kB raw / <2 kB gzip per typical
page, one cached sheet <=20 kB, header never blocks a page, and the 1.3 GB Vite process gone.**

## V2 layout (concrete)
```
<head>  title, theme-color, 1 stylesheet (/_ui/console.<hash>.css); the icon sprite is fetched once by <use> and cached
<body class=app>
  <header class=top>  logo <use>, page title + one-line lede, origin dot (cached), theme toggle (2 links),
                      <details class=menu> room sheet: 28 rooms as <a> + <svg><use href="/_ui/i.<hash>.svg#id"/></svg>
  <main id=main>      page body only (tables, cards, forms). Forms POST + 303 redirect (PRG), no JS.
  <nav class=dock>    5 primary rooms (as today)
```
- **Icon sprite**: IMPLEMENTED (slot 276): `icons::PATHS` rendered once as `/_ui/i.<fnv64>.svg` (`<symbol id=..>`), immutable cache, same
  origin so the current CSP (`img-src 'self'`) holds; pages use `<svg class=i><use href=".../i.<h>.svg#desk"/></svg>`
  (~70 B instead of ~220 B each). Expected: -8 kB per page (~55% of a typical page).
- **Compression**: gzip HTML when the client sends `Accept-Encoding: gzip` (5.6x measured); precompress the CSS and
  sprite once at start. Dependency: `miniz_oxide` (MIT OR Zlib OR Apache-2.0, pure Rust, no C) - only if the owner
  accepts one dep; otherwise skip HTML gzip and rely on the sprite cut.
- **Header dot off the hot path**: IMPLEMENTED (slot 276): one shared health value refreshed at most every 5 s (single-flight, `Mutex` +
  `Instant` via `Ui::cached_health` / `HEALTH_CACHE_TTL`), so static rooms (`/agents /data /jupyter /gateway /memory /node /security /analytics /guardian`) answer
  in ~1 ms instead of one upstream RTT. Rooms needing data keep their parallel fan-out (`std::thread::scope`, already
  used in lib.rs) with a 1.5 s per-call timeout and a "stale/did not load" line.
- **CSS split**: base sheet (RULES + PORT1-4 ~19 kB) for every page; `ov-*` (~6 kB) as `/_ui/overview.<h>.css` linked
  only on `/console`. Emit the light palette once (`html[data-theme=light],` + media query sharing one rule via
  `:where()`). Base sheet budget **20 KiB**, overview 8 KiB; test asserts both.
- **Templating**: keep hand-written `format!`/`esc()` (no dependency, 630 call sites work today) but move each page to
  one `fn render(&Data) -> String` with a typed `Data` struct, so pages are testable without HTTP. Revisit `maud` only
  if markup bugs show up (see research).
- **Interactivity**: none added. Every action stays a plain form POST with PRG. No htmx in V2 (keeps CSP
  `script-src` absent). If live refresh is ever wanted, use `<meta http-equiv=refresh>` on `/status` only.

## Per-route cuts
| route | today raw / gz | V2 cut | target raw |
|---|---|---|---|
| all 29 chrome pages | 14.2-31.9 kB | sprite (-8 kB), gzip, cached health dot | -8 kB each |
| `/console` | 31.9 / 7.6 kB | overview CSS split; the 15 kB of inline SVG includes the map: render map once as `/_ui/map.<h>.svg` if static, else keep | ~12 kB |
| `/status` | 3.6 / 1.1 kB | already the model page (no full chrome); keep | 3.6 kB |
| `/services /repos /docs /cloudflare` | 19-21 kB | sprite; tables only, drop repeated per-row icons (use CSS dot) | ~6-11 kB |
| `/agents /data /jupyter /gateway /memory /node /analytics` | 14-16 kB, main 0.3-1.9 kB | static: sprite + cached dot -> ~1 ms live | ~5-7 kB |
| `/guardian /keys /security /session` | 14.5-16.3 kB | show one "sign in to see this" card instead of each 401 tile | ~5-6 kB |
| `/cache /hermes /context` | 21-28 kB | paginate lists (first 50 rows, `?page=`), sprite | ~10-12 kB |
| `/instances /keys /markers` | render 7 ms | single-pass formatting, no re-escape of already-escaped parts | <2 ms |
| `/arena /bank` | Rust page, SPA_FIRST when fallback set | see ruling R2 | - |
| `/soon`, `/md/$name` | 0.8-0.9 kB | keep | - |

## Open rulings (owner)
- **R1 CSS budget.** IMPLEMENTED (slot 254): base sheet <=20 KiB + overview <=8 KiB,
  asserted in `css.rs` tests; `/console` links `/_ui/overview.<h>.css`. Light palette uses
  `:where(html:not([data-theme]))` under the system media query. Dead PORT1 `.road` dropped
  (PORT2 owns it). `/hermes` removed from SPA_FIRST (Rust-first).
- **R2 SPA/JS for `/arena`, `/bank`, `/hermes`.** `/hermes` SPA_FIRST dropped (slot 254;
  Rust page reads history live). Still open: keep `/arena` read-only in Rust (paid Breed
  stays out until Slavik approves spend); `/bank` already says "no bank API yet" — drop
  SPA_FIRST for both once the Vite process is retired. Consequence: zero JS on all routes.
- **R3 stale `real_console_ui`.** It asserts "Not in Rust yet" rooms that no longer exist. Recommend: delete it and
  keep `real_merge` as the single live route test (move its ports to a config env so other slots can reuse it).
- **R4 (new) HTML gzip dependency.** `miniz_oxide` yes/no.
- **R5 (new) Retire the Vite dev server on :8080.** It is a 1.3 GB dev process serving production. Needs the Rust
  API surface listed in the measurement doc. Live service change - Slavik's call and a separate cutover slot.

## Phases
1. **Front-only cuts (no VPS change, ~1 slot):** sprite + cached health dot (slot 276), CSS split + budget (slot 254), light palette once,
   delete `real_console_ui`, `real_merge` ports via env. Gate: all 30 routes 200, 0 `<script>`, median page <=7 kB,
   static rooms <=10 ms live through the forward after first warm-up.
2. **Compression (if R4 yes):** gzip HTML + precompressed assets. Gate: gzip page <=2.5 kB, CPU per page <=1 ms.
3. **API in Rust:** serve the listed `/api/v1`, `/api/v3/guardian`, `/api/hermes`, `/api/mesh`, `/api/arena`
   endpoints from existing Rust services (desk, hermes-rs, nerve) or a thin `console-api` crate; `/_serverFn/*`
   dropped. Gate: every Rust page renders the same data with upstream = Rust API.
4. **Cutover (R5, owner-approved slot):** run the Rust front on :8080 in place of `vite dev`; rollback = restart
   console.service. Expected memory saving ~1.29 GB on the VPS.

## Research: reuse before writing (MIT-compatible only)
| option | licence | size / deps | verdict |
|---|---|---|---|
| keep `format!` + `html::esc` | ours | 0 deps | **V2 default** - works, 0 deps, render <=10 ms |
| `maud` 0.27 | MIT OR Apache-2.0 | proc-macro (`maud_macros`, syn/quote at build), `itoa` | best fallback if hand-written markup gets buggy: compile-time HTML, auto-escape |
| `askama` 0.16 | MIT OR Apache-2.0 | derive macro, `itoa` | good, but templates in separate files; heavier build |
| `markup` 0.16 | MIT/Apache-2.0 | proc-macro | smaller community |
| `hypertext` 0.12 | MIT | proc-macro, 56 kB crate | maud-like, few users |
| `sailfish` 0.11 | MIT | proc-macro + runtime | fastest, more machinery than we need |
| `horrorshow` 0.8 | MIT/Apache-2.0 | macro_rules only | fine, older style |
| `minijinja` 3 | **Apache-2.0 only** | runtime engine 228 kB | allowed by our list but not MIT; runtime templates not needed |
| `miniz_oxide` 0.9 | MIT OR Zlib OR Apache-2.0 | pure Rust | **pick for gzip** (R4) |
| `flate2` 1.1 | MIT OR Apache-2.0 | wraps miniz_oxide | not needed, use miniz_oxide directly |
| htmx 2 (JS) | 0BSD | ~14 kB gz JS | not in V2 (keeps zero-JS CSP) |
| datastar (JS) | MIT | JS + SSE | not in V2 |
| fixi (JS) | no licence file found | - | **not allowed** |
| Lucide icons | ISC | SVG | only if new icons are needed; current paths are ours (from console-ide `icon.tsx`) |
| Tabler icons | MIT | SVG | alternative to Lucide |
Sources: crates.io API and GitHub licence files, read 2026-10-22 01:10 +02:00. No dependency added in this slot.
