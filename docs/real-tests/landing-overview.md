<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: Desk landing overview (`/console`)

Box only, no VPS contact. Slot 40 ports: binary on 127.0.0.1:19402 (test) / 19400 (manual view), upstream 127.0.0.1:19403 / 19401 closed on purpose
(the overview must render with the API down).

## Rust test
`GALION_REAL=1 cargo test --release -p console-static --test real_landing -- --nocapture`
(crates/console-static/tests/real_landing.rs: starts the release binary, probes, kills it.)

Run 2026-10-20 23:00:59 CEST (+02:00):
```
GET /console -> 200, 31667 bytes, 1 ms, 0 <script>, 9 cards, 2 svg
GET /_ui/console.e5c80f89db4f891e.css -> 200, 19251 bytes
GET / -> 302 Location: /console
GET /status -> 200
GET /services -> 200
GET /agents -> 200
GET /docs -> 200
GET /soon -> 200
GET /hermes -> 200
GET /console (cookie light) -> data-theme=light
test real_landing_overview ... ok
```

## Manual run (for screenshots)
2026-10-20 22:59 CEST (+02:00): `galion-console-static --ui rust --upstream 127.0.0.1:19401 --bind 127.0.0.1:19400 --quiet`;
`curl /console` 200 (31 660 bytes), 0 `<script>`; `/` 302 → /console; /status, /services, /agents, /docs, /hermes, /soon all 200;
RSS 2 676 kB. Headless Chrome screenshots in /workspace/swarm/artifacts/landing/. Process stopped afterwards.

## Hermetic tests
- `crates/console-static/tests/ui.rs::landing_overview` (fake API): 200, all 9 sector cards and 3 sections, no script, no inline style,
  CSP header, system theme with no cookie, cookie light/dark honoured, only `GET /api/v1/health` reaches the API, `/` redirect,
  HEAD 200, POST 405, non-console host gets the quiet page.
- `read_rooms_render_get_only`: every PORTED page (now including /console) is script-free and GET-only; 16 rooms remain "Not in Rust yet" (was 17).
- `pages::overview` unit tests (sections, balanced tags, no handler attributes, bar scaling) and `overview` data tests (plan totals add up, 52 systems).
