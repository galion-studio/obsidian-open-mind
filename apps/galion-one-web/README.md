<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Galion One V2 on one.galion.app (owner order t428u, 2026-10-30)

## How one.galion.app is served
- cloudflared ingress: `one.galion.app` goes to `galion-one.service` on loopback :8091. The rule `one.galion.app` + `^/tanga(/.*)?$` goes to tanga-proto :8671, and is unchanged. **`galion.app` and `www.galion.app` use the same :8091**, so they show the same page.
- `galion-one` (`/opt/galion/rust/bin/galion-one`, `crates/galion-one`) reads `$GALION_HOME/one/index.html` (`/workspace/galion-light` resolves to `/opt/galion/galion-light`) **on every request**. It inserts the loading screen `#galion-boot` straight after `<body>`. The loading screen has the icon SVG, "Galion One" and "Obsidian Open Mind Stack" (`one_page()` in main.rs).

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
- The theme-color is set to `#05060f`.
- No new routes, no JS changes, no external assets. It works with JS off the same way as before.

## Build
`./apply-v2.sh <live one/index.html> <out>` (POSIX sh + awk + sed). From the live page sha256 `d9852aaf…0620`, it produces `20439662…ca61`.

## Deploy / rollback
See `/workspace/swarm/reports/galion-one-v2-20261030.md` §8. You only swap the file; no restart is needed.
