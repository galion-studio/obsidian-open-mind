<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party material: swarm/console-ui-rust

Written 2026-10-20 21:25 +02:00.

## New dependencies

**None.** `crates/console-ui` has an empty `[dependencies]` table (std only); `crates/console-static` gains only the
path dependency on `console-ui`. `Cargo.lock` gains one entry (`console-ui`, workspace-local, MIT). No crates.io,
npm, CDN or font downloads are involved at build time or at run time.

| name | version | licence (SPDX) | source | used for | why not written ourselves |
|---|---|---|---|---|---|
| (none) | | | | | |

Written here instead of pulled in, so the dependency count stays at zero: a strict JSON reader/writer
(`json.rs`, instead of serde_json), a loopback HTTP/1.1 client with chunked decoding (`client.rs`, instead of
ureq/reqwest), the stylesheet (`css.rs`, instead of Tailwind v4, whose compiler is JavaScript).

## Material carried over from Galion's own console-ide (not third-party)

All from `/opt/galion/console-ide` (Galion-owned, MIT like this repo), live working tree read-only on
2026-10-20 21:05 +02:00; each Rust file names its source file in its module comment.

| what | from | into |
|---|---|---|
| SVG icon path data (49 icons) and the Galion mark | `src/components/icon.tsx`, `src/components/mark.tsx` | `crates/console-ui/src/icons.rs` |
| design tokens (colours, radii, font stacks) | `src/styles.css` `@theme`, `src/components/ide-shell.tsx` `LIGHT` | `crates/console-ui/src/css.rs` |
| room list and dock | `src/lib/rooms.ts`, `src/components/ide-shell.tsx` | `crates/console-ui/src/rooms.rs` |
| gate rules and page texts (PAIR, SOON, LINKS) | `src/start.ts`, `src/lib/host.ts`, `src/components/phone-gate.tsx` | `crates/console-ui/src/gate.rs`, `pages/gate.rs` |
| page copy and grouping logic | `src/routes/status.tsx`, `src/routes/_console/{services,agents}.tsx`, `src/lib/fleet.ts` | `crates/console-ui/src/pages/*.rs` |

## Not shipped

- Font files: the stylesheet names "IBM Plex Sans/Mono" (SIL OFL-1.1) in `font-family` stacks exactly as the TSX did;
  no font file is bundled or fetched (the TSX loaded them from Google Fonts; the Rust pages do not, the browser falls
  back to installed fonts). CSP `default-src 'none'; style-src 'self'` blocks remote fonts by design.
- Tailwind, React, TanStack, zustand, class-variance-authority: replaced, nothing copied.
