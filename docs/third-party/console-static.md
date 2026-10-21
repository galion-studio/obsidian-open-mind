<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: console-static (slot 34, B61)

Written 2026-10-20T20:50+02:00 (Europe/Zagreb).

## Rust (what this branch adds)

`crates/console-static` has **no dependencies** (std only,
`#![forbid(unsafe_code)]`). No code was copied. The startup-whitelist idea
follows `crates/tuner` (same repo, MIT). Nothing new appears in Cargo.lock
except the crate itself.

Why not galion-common (`swarm/common` 26d98b0): its HTTP server is buffered
request -> buffered response; the console needs a streaming reverse proxy
(SSE on `/api/v1/markers/stream`, hermes run events) with socket-level
control. Merging it would also pull the tuner-cutover chain (common touches
tuner). Head parsing can move onto galion-common once common is on main.

## No JS tooling on this branch

Slavik's standing rule of 2026-10-20 20:41 +02:00: Rust only, Node/npm/Vite
not allowed even as box build tools. The branch contains no Node script, no
Vite config and no package files. `routes.txt` generation is Rust
(`galion-console-static gen-routes`).

## Provenance of the pre-built asset set used in tests (not committed)

The asset set at `/workspace/swarm/artifacts/console-static/out/client` was
built at 2026-10-20T20:36:44+02:00, **before** the rule, on the box with Node
v20.19.2 / npm 11.21.0 / Vite 8.3.4 (MIT) / rolldown 1.2.14 (MIT) / TanStack
Start 1.169.3 (MIT) from console-ide 2169d1c's own `package.json` (no lockfile
is tracked there). Nothing ran on the VPS. Its installed package list with
licence fields is kept as a record in `docs/third-party/console-static-npm.tsv`
(425 packages): MIT 352 (+1 MIT AND ISC), ISC 27, Apache-2.0 22, BSD-3 7,
BSD-2 6, 0BSD 1, and outside the allowlist: lightningcss MPL-2.0 (4 entries),
lru-cache and minimatch BlueOak-1.0.0, caniuse-lite CC-BY-4.0, isbot
Unlicense, argparse Python-2.0 (a JS package; no Python ran).

None of the non-allowlisted packages appears in the 36 shipped files (grep
for each name: 0 hits); they are compiler/linter dependencies. Shipped code
comes from the console's runtime `dependencies` (react / react-dom 19,
@tanstack/*, Radix UI, lucide-react ISC, zustand, clsx, tailwind-merge, ...),
all MIT, ISC or Apache-2.0 in the TSV.

Whether this asset set (or any JS-built one) may be served is the open
question for Slavik (see docs/console-static.md, "The asset set").
