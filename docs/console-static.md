<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Console static front (B61, swarm/console-static)

- Written: 2026-10-20T20:50+02:00 (Europe/Zagreb)
- Slot 34, ports 19340-19349, base main 13a296c
- Live console: `console.service`, VPS 127.0.0.1:8080, Vite **dev** server,
  `/opt/galion/console-ide` HEAD 2169d1c **plus ~60 uncommitted/untracked files**
- Status: built and tested on the box. **Nothing installed or changed on the VPS.**
  The unit below is a staged proposal that needs Slavik's approval.

## Why (what the dev server exposes today)

Read-only GETs against 127.0.0.1:8080 on 2026-10-20T20:28+02:00 (status only,
bodies discarded):

| path | live 8080 | meaning |
|---|---|---|
| `/package.json` | 200 application/json | project manifest served |
| `/vite.config.ts` | 200 text/javascript | build config source served |
| `/src/lib/host.ts` | 200 text/javascript | **any source file under src/ is served (transformed), including `*.server.ts`** |
| `/@vite/client` | 200 | dev runtime |
| `/node_modules/.vite/` | 307 | dev cache |
| `/.git/config`, `/.git/HEAD`, `/@fs/opt/galion/console-ide/.git/config` | 403 | blocked only by Vite's fs.deny |
| `/.env`, `/public/.env`, `/phpinfo.php` | 404 | scanner probes seen in the nerve capture |

The nerve capture (`/workspace/swarm/nerve-capture`, replay1) flags this as
"scanner/exploit probes against console" and "a dev server is answering
internet traffic". Serving `src/**/*.server.ts` means server-side code
(auth, env handling, connector calls) is readable by anyone who reaches the
origin. Values in `.env` are not served, but the code paths are.

## What the console is

TanStack Start (React 19, Vite 8) full-stack app, not a static site:

- 16 page routes at 2169d1c (31 in the live working tree), client-rendered;
- ~27 server routes under `/api/v1/*` and `/api/hermes/*` at 2169d1c (54 live,
  plus `/api/arena`, `/api/mesh`, `/api/v3/guardian/*`), implemented in Node
  (catalog payloads, `os` stats, hermes proxy with auth, pglite/better-auth);
- one server function (`createServerFn`, connector readiness) -> `/_serverFn/*`;
- a phone/host gate middleware in the untracked `src/start.ts` (decides by
  Host; measured: console.galion.app on `/api/` -> 401 `{"error":"phone required"}`,
  galion.app -> 404 `{}`, loopback -> 200).

So "fully static" covers the pages and assets. The API stays a separate
upstream until it is ported.

## The production path

```
cloudflared ──> 127.0.0.1:8080  galion-console-static (Rust, std only, ~0.5 MB, ~2.3 MB RSS)
                  ├─ built SPA files (dist/client, whitelist indexed at start)
                  ├─ page routes -> one prerendered HTML shell
                  ├─ /api/*, /_serverFn/* -> 127.0.0.1:8088 (API upstream, streamed, Host kept)
                  └─ everything else -> 404
```

### The front (`crates/console-static`, binary `galion-console-static`)

- **Whitelist, not filesystem lookups**: `dist/client` is walked once at start.
  Listed: regular files only, symlinks skipped (the root itself may be a
  `current -> rev` link), every segment clean, extension with a known MIME.
  Requests are looked up in that map; a request path is never joined to disk.
- **Strict paths** (`path.rs`): origin-form only; `%XX` decoded once; encoded
  `/` or `\`, double encoding (`%25`), NUL/control bytes, non-ASCII, `//`,
  `.`/`..` and **any segment starting with `.`** are refused with 404.
- **No dotfiles, no listing**: by construction (never indexed; directories are
  not entries). `/_shell.html` is not reachable by its own name.
- **MIME**: fixed table; unknown types (`.php`, `.map`, `.ts`, no extension)
  are never served.
- **Cache**: `/assets/*-HASH.*` -> `public, max-age=31536000, immutable`;
  HTML shell and `manifest.webmanifest` -> `no-cache`; other files ->
  `public, max-age=3600`; ETag + `If-None-Match` -> 304; errors `no-store`.
  `X-Content-Type-Options: nosniff`, `Referrer-Policy` on everything.
- **Routes** (`routes.txt`, generated from the route tree by the build):
  `page /agents`, `page /md/$name` (one segment), `redirect / /console`.
  Unknown paths 404 (live TanStack also 404s them).
- **Proxy** (`proxy.rs`): only configured prefixes (default `/api/`,
  `/_serverFn/`), only after the path passed the strict check (so
  `/api/../.env` and `/api/.env` never reach the upstream); methods
  GET/HEAD/POST/PUT/PATCH/DELETE/OPTIONS; hop-by-hop headers stripped;
  `Host` forwarded unchanged (the phone gate needs it); `X-Forwarded-For/
  Proto/Host` set (client-supplied `X-Forwarded-Host` dropped); Content-Length
  bodies up to 25 MiB (hermes transcribe), chunked uploads -> 411; response
  streamed with per-read flush (SSE works); upstream must be a loopback IP.
- **Limits**: 16 KiB head, 64 headers, duplicate Host/Content-Length rejected,
  15 s idle, keep-alive up to 100 requests, 256 concurrent connections (503
  beyond), access log without query strings.
- Loopback bind enforced unless `--allow-public-bind`.

### The asset set (no JS tooling: Rust-only rule, Slavik 2026-10-20 20:41 +02:00)

The front serves an **already-built** asset set and never builds anything.
The set used for every test here is `/workspace/swarm/artifacts/console-static/out/`
(`client/` 36 files, 871 756 B, + `routes.txt`, `BUILD-INFO.txt`,
`npm-licences.tsv`). It was produced at 2026-10-20T20:36:44+02:00 from a
`git archive` of console-ide 2169d1c (tracked files only) with Vite 8.3.4 in
TanStack Start SPA mode, **before** the Rust-only rule; Node/npm/Vite are no
longer used and the Node build script and Vite config were removed from this
branch. The asset set is not committed (build output of a separate repo).

`routes.txt` is made in Rust: `galion-console-static gen-routes
src/routeTree.gen.ts [--redirect / /console]` reads the TanStack route tree
(a generated file tracked in console-ide) and prints the page list; it
reproduces the committed `deploy/console-static/routes.2169d1c.txt` exactly.

**Needs Slavik's ruling: the browser UI itself is TypeScript/TSX** (React 19,
TanStack Router/Start, Tailwind v4, ~425 npm packages). A fresh asset set
cannot be produced today without a JS runtime. Options:

- (a) allow serving asset sets built elsewhere (e.g. the existing one, or one
  built in the console-ide repo's own CI), treating them as opaque files;
- (b) a Rust-only build: per-file TS/JSX strip with oxc or swc (MIT /
  Apache-2.0 crates) plus a Rust bundler, a Rust fetcher for the npm tarballs,
  and a replacement for the Tailwind v4 CSS compiler (its engine is JS; only
  the class scanner "oxide" is Rust). Not attempted here; a multi-day job with
  real risk of mismatched output;
- (c) rewrite the UI in Rust/plain HTML (largest).

### Phases for the API upstream

1. **Phase 1 (this branch, staged)**: keep the current console process as
   the API upstream, moved to 127.0.0.1:8088 (`console-upstream-8088.conf`).
   Only `/api/*` and `/_serverFn/*` reach it; source files, `/@vite`, `/@fs`,
   `/.git`, `/package.json` are 404 at the front. This closes B61's exposure
   with one drop-in and one new unit, no rebuild on the VPS. It keeps the
   Node process that already runs today as the API upstream (nothing new in
   Node); under the Rust-only rule phase 3 is the end state.
2. **Phase 2**: replace the dev server upstream with a non-dev API process
   (T6's staged nitro bundle exists in the openmind tree; that is Node at
   runtime, so it needs Slavik's call given "no Node on the VPS").
3. **Phase 3**: port the API to Rust (`openmind-console-api`): most v1 routes
   are static catalog payloads (`src/lib/catalog.ts`), `/api/v1/pod` is `os`
   stats, the hermes routes proxy to `HERMES_URL` behind the user gate. Then
   no Node at all on the VPS.

## Cutover proposal (operator only, after approval; nothing here was run)

1. Commit the live console-ide working tree to a rev (it has ~60 modified or
   untracked files, including `src/start.ts`, the phone gate and 15 routes the
   2169d1c asset set does not have) and obtain an asset set for that rev under
   whatever Slavik rules above. Generate `routes.txt` with `gen-routes` and
   compare with `deploy/console-static/routes.live-20261020.txt` (add
   `--redirect / /console` while `/console` is the landing page).
2. Build `galion-console-static` for x86_64 musl on the box; copy binary to
   `/opt/galion/bin/`, `OUT/client` + `routes.txt` to
   `/opt/galion/console-static/<rev>/`, link `current -> <rev>`.
3. Copy `/etc/systemd/system/console.service` aside. Install
   `console-upstream-8088.conf` as a drop-in and `console-static.service`.
4. `systemctl daemon-reload; systemctl restart console` (now on 8088), then
   `systemctl enable --now console-static` (takes 8080). Downtime: seconds.
5. Verify with GETs: `crates/console-static/tests/real_console_static.rs` style checks against
   127.0.0.1:8080: pages 200, every probe 404, and `/api/v1/health` answers
   exactly as the console did per Host (measured through the front on
   2026-10-20T20:41+02:00: Host 127.0.0.1 200 JSON, Host console.galion.app
   401 `{"error":"phone required"}` 26 B, Host galion.app 404 `{}`; identical
   to direct 8080, so the gate in `src/start.ts` is preserved).
6. Rollback: `systemctl disable --now console-static`, remove the drop-in,
   `daemon-reload`, `restart console` (back on 8080).

## Differences vs live (2169d1c build, measured)

- `/` is 200 (index page) on the 2169d1c build; live is 302 -> `/console`
  (route from the uncommitted tree). The live route file carries the redirect.
- Pages are a client-rendered shell (same for every page) instead of SSR HTML;
  the browser renders the same UI (headless Chrome dump of /services, /agents,
  /status through the front showed the console UI and live API data).
- `/?install=1` PWA tutorial and manifest/head middleware from the untracked
  `server/` dir (nitro middleware) are not reproduced; head tags in
  `__root.tsx` still give icon/manifest/theme.
- Non-console Host on pages: SSR redirect becomes a client redirect to `/soon`
  (`_console.tsx` beforeLoad checks `document.location.host`).
- `/auth/popup` is a dev-only middleware; deployed apps use full-page OAuth.

## Rust pages (swarm/console-ui-rust, 2026-10-20 21:20 +02:00)

`--ui rust` renders `/status`, `/services` and `/agents` in Rust (crates/console-ui) with no JavaScript; see
`docs/console-ui-decision.md` for the decision record and `docs/real-tests/console-ui-rust.md` for the live
comparison. The asset-set question above is narrowed to the rooms not yet ported.
