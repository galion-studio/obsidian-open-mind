<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Galion One V2 on one.galion.app (owner order t428u, 2026-10-30)

## How one.galion.app is served
- cloudflared ingress: `one.galion.app` goes to `galion-one.service` on loopback :8091. The rule `one.galion.app` + `^/tanga(/.*)?$` goes to tanga-proto :8671, and is unchanged. **`galion.app` and `www.galion.app` use the same :8091**, so they show the same page.
- `galion-one` (`/opt/galion/rust/bin/galion-one`, `crates/galion-one`) reads `$GALION_HOME/one/index.html` (`/workspace/galion-light` resolves to `/opt/galion/galion-light`) **on every request**. It inserts the loading screen `#galion-boot` straight after `<body>`. The loading screen has the icon SVG, "Galion One" and "Obsidian Open Mind Stack" (`one_page()` in main.rs).

## Host scope (t428u card answer)
V2 shows on **one.galion.app only**. A tiny inline head script adds `class="g1v2"` on `<html>` when `location.hostname === "one.galion.app"`. It runs before first paint, so nothing flashes. Every v2.css rule is scoped under `html.g1v2`, and `.g1-sky` is `display:none` otherwise. On galion.app and www.galion.app nothing matches, so they render exactly as the pre-V2 page: same pixels and the same theme-color #ffffff.

## What V2 changes (page only; the binary, the loading-screen words and the icon are untouched)
- `v2.css` is inlined before `</head>`:
  - The room gets the V2 night sky with three slow light fields that follow the voice state (listening teal, thinking orchid, speaking amber).
  - A soft light orb sits behind the unchanged Galion One mark.
  - V2 system type.
  - One look only, so the White/Dark switch is hidden.
- The loading screen is restyled with higher-specificity selectors and keeps the same words and icon:
  - It is transparent, so it sits on the same sky as the room.
  - It uses the same font and the same orb, which rises in behind the icon.
  - At hand-off it lifts and blurs away while the room glides in (opacity only, on the V2 glide curve).
- `prefers-reduced-motion` stops every loop.
- The theme-color is set to `#05060f` (one.galion.app only).
- No new routes, no JS changes, no external assets. It works with JS off the same way as before.

## Build
`./apply-v2.sh <live one/index.html> <out>` (POSIX sh + awk + sed). From the live page sha256 `d9852aaf…0620`, it produces `85975b91…711f`.

## Deploy / rollback
See `/workspace/swarm/reports/galion-one-v2-20261030.md` §8. You only swap the file; no restart is needed.

## Full V2 page (t431u/t432u, 2026-10-30)

- **Source:** `v2/` (style.css, body.html, app.js, mic-worklet.js). Run `v2/build.sh` to make `v2/index-v2.html`, then install it as `/opt/galion/galion-light/one/index-v2.html`. No restart is needed: galion-one reads the file on each request.
- **Hosts:** galion-one serves `index-v2.html` only to hosts in `ONE_V2_HOSTS` (default `one.galion.app`). Every other host keeps `one/index.html`.
- **Routes the page uses:**
  - existing: `/me`, `/pair`, `/admin/say`, `/session`, `/files`, `/model`, `/models`, `/voice`, `/api/v1/awake`;
  - new: `/pair/invite`, `/stt/health` and `/stt/stream` (WebSocket proxy to galion-stt, `GALION_STT`, default 127.0.0.1:8651).
- **Pairing a new phone:**
  - from a phone that is already paired: Devices → Pair another phone;
  - on the server: `galion-one pair-invite [minutes]` (it prints the code; keep it out of logs and chat).
- **ntfy topic:** read from `ONE_NTFY_ENV` (default `/etc/galion/ntfy-relay.env`, `NTFY_URL=https://ntfy.sh/<topic>`). Only a paired device sees it, next to its invite code.
- **Rollback:** rename `index-v2.html` to `index-v2.html.off`.
