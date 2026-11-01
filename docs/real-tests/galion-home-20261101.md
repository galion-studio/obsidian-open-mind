<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: galion.app home page (swarm/galion-home-20261101)

Run: 2026-11-01 14:38 +01:00 (Europe/Zagreb). Local only, ports 19980-19985, nothing deployed.

- `cargo test -p galion-one`: 103 passed, 1 ignored (incl. 5 `home::` tests); clippy `-D warnings` clean; release musl build ok (sha256 prefix 82f6c2a50e89db9b).
- curl, Host: galion.app: `/` 200 gzip 9,410 B (ETag, 304, CSP, Vary), `/account` 200 no-store, `/pricing` 301 -> `/#pricing`, `/home.js` 200, `/galion-mark.svg` 200, `/font/jost.woff2` 200. `HEAD /api/checkout?plan=plus` 404 locally and on live galion.app, so buttons stay "Coming soon".
- Host: one.galion.app / localhost: unchanged One app.
- `ONE_CHECKOUT=on`: 2 Subscribe links, "not open yet" note removed. `ONE_CHECKOUT=probe` with a mocked 204: both buttons become Subscribe (WebKit, Firefox).
- Playwright, Chromium / WebKit / Firefox at 390x844 and 1440x900: status 200, no sideways scroll, 0 console errors, ~23-24 KB transferred in 3-4 requests (live page before: ~108 KB, 11 requests).
