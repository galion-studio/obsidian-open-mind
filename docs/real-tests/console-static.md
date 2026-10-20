<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-static (slot 34, ports 19340-19343)

All times Europe/Zagreb (+02:00). Box only; the VPS saw read-only GETs (direct
`curl` over ssh, and an `ssh -L 127.0.0.1:19341:127.0.0.1:8080` port forward
that writes nothing on the VPS). All started processes were killed after each run.

## 1. Live 8080 baseline (2026-10-20T20:28:28+02:00, GET, status only)

```
/                                   302 -> /console
/agents ... /services, /soon        200 text/html (SSR)
/status                             200 text/html
/favicon.svg /manifest.webmanifest /og.jpg /__grok/icon-180.png   200
/nope                               404 text/html
/api/v1/health catalog services protocol markers, /api/hermes/health   200 application/json
/.env /public/.env /phpinfo.php /public/phpinfo.php                    404
/.git/config /.git/HEAD /@fs/opt/galion/console-ide/.git/config        403 (Vite fs.deny)
/package.json                       200 application/json   <- dev server leak
/vite.config.ts                     200 text/javascript    <- dev server leak
/src/lib/host.ts                    200 text/javascript    <- any src file served
/@vite/client                       200 text/javascript
/node_modules/.vite/                307
```

## 2. Asset set (built on the box BEFORE the 20:41 Rust-only rule; Node tooling since removed)

```
built: 2026-10-20T20:36:44+02:00
node: v20.19.2  npm: 11.21.0
vite: 8.3.4
files: 37  bytes: 871756
[prerender] Prerendered 1 pages: /     (SPA shell -> _shell.html)
routes.txt: 16 pages
```
Bundle scan: no `process.env`, no env var names, no key-shaped strings in `dist/client`.

## 3. `scripts/real/console-static.sh` with `CONSOLE_STATIC_LIVE=1` (2026-10-20T20:38:21+02:00)

Release binary 406 616 B (x86_64 gnu; musl 512 800 B, aarch64 musl 448 104 B), front RSS 2.3 MB.

```
path                                         ours (code type | cache)                 live
/                                            200 text/html; charset=utf-8 | no-cache  302 
/soon                                        200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/status                                      200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/agents                                      200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/chill                                       200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/cloudflare                                  200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/docs                                        200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/domains                                     200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/gateway                                     200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/hermes                                      200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/jupyter                                     200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/markers                                     200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/node                                        200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/pack                                        200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/repos                                       200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/services                                    200 text/html; charset=utf-8 | no-cache  200 text/html; charset=utf-8
/favicon.svg                                 200 image/svg+xml | public, max-age=3600 200 image/svg+xml
/manifest.webmanifest                        200 application/manifest+json | no-cache 200 application/manifest+json
/og.jpg                                      200 image/jpeg | public, max-age=3600    200 image/jpeg
/__grok/icon-180.png                         200 image/png | public, max-age=3600     200 image/png
/__grok/install/styles.css                   200 text/css; charset=utf-8 | public, max-age=3600 200 text/css
/assets/_console-BgfJLO6q.js                 200 text/javascript; charset=utf-8 | public, max-age=31536000, immutable 404 text/html; charset=utf-8
/assets/styles-CL6A0KsY.css                  200 text/css; charset=utf-8 | public, max-age=31536000, immutable 404 text/html; charset=utf-8
-- scanner probes (nerve-capture + dev-server leaks): all must be 404
/.env                                        404
/public/.env                                 404
/phpinfo.php                                 404
/public/phpinfo.php                          404
/.git/config                                 404
/.git/HEAD                                   404
/@fs/opt/galion/console-ide/.git/config      404
/opt/galion/console-ide/.git/config          404
/package.json                                404
/vite.config.ts                              404
/src/lib/host.ts                             404
/src/lib/env.server.ts                       404
/@vite/client                                404
/node_modules/.vite/                         404
/%2e%2e/%2e%2e/etc/passwd                    404
/.vercel/project.json                        404
/_shell.html                                 404
/wp-login.php                                404
/.aws/credentials                            404
/server-status                               404
/nope                                        404
/assets/                                     404
/health                                      404
-- API GETs through the front vs direct live (read-only)
/api/v1/health                               ours 200 application/json         live 200 application/json         same-body
/api/v1/catalog                              ours 200 application/json         live 200 application/json         same-body
/api/v1/services                             ours 200 application/json         live 200 application/json         same-body
/api/v1/protocol                             ours 200 application/json         live 200 application/json         same-body
/api/v1/markers                              ours 200 application/json         live 200 application/json         same-body
/api/hermes/health                           ours 200 application/json         live 200 application/json         same-body
/api/nope                                    ours 404 text/html; charset=utf-8 live 404 text/html; charset=utf-8 body-differs(2928/2928)
/api/.env                                    ours 404 text/plain; charset=utf-8 live 404 text/html; charset=utf-8 body-differs(10/2928)
-- method checks
POST /agents -> 405
POST /.env -> 404
If-None-Match favicon -> 304
real: access-log 404 lines 26
real: end 2026-10-20T20:38:51+02:00 fail=0
```

Pages and static files match live (200, same types); `/` differs because live
has the uncommitted `/console` route (staged `routes.live-20261020.txt` carries
`redirect / /console`); `/assets/*` 404 on live is expected (dev server has no
build output). API GETs through the front returned **byte-identical bodies** to
direct 8080 for 6/6 JSON routes. `/api/.env` is refused at the front (never
reaches the upstream).

SSE: in run 1 (20:37:30) `GET /api/v1/markers/stream` through the front
streamed the first `heartbeat` frame within the 3 s cap. Note: that handler
calls `heartbeatMarker()`, which bumps an in-memory marker sequence on the live
console (42 -> 43 across the two runs). It is a GET, but it is not side-effect
free, so the script now only does it with `CONSOLE_STATIC_SSE=1`.

## 4. Host gate through the front (2026-10-20T20:41:55+02:00, GET /api/v1/health)

```
Host galion.app           front 19340 -> {} [404 2B]                         direct -> {} [404 2B]
Host console.galion.app   front 19340 -> {"error":"phone required"} [401 26B] direct -> same [401 26B]
Host 127.0.0.1:19340      front -> 200 JSON (section 3)
```

## 5. Headless Chrome through the front (live API via tunnel)

`google-chrome --headless=new --dump-dom` on /services, /agents, /status:
rendered the console UI ("Galion Console 3.4.0-cpu · Services · P42 origin ...",
"Operator agents · no VRAM ...", "Origins are dark. 9 subsystems online ...").
Browser requests seen in the front's access log: only GETs (assets, manifest,
`/api/v1/health` x3, all 200; manifest revalidated 304). `/` was deliberately
not loaded against live because its page POSTs a marker.

## 6. Rust real-binary test (GALION_REAL=1, port 19342)

```
$ GALION_REAL=1 cargo test -p console-static --release --test real_console_static -- --nocapture
captured scanner paths: ["/public/.env", "/public/phpinfo.php", "/opt/galion/console-ide/.git/config"]
test real_binary_serves_console_and_404s_probes ... ok
```
The scanner paths are parsed from `/workspace/swarm/nerve-capture/*.jsonl`
(Vite's "Instead of ..." and "request id ... outside of Vite serving allow
list" lines) and replayed with the fixed list (`/.env`, `/phpinfo.php`,
`/.git/config`, `/@fs/...`, source files): all 404.

## 7. After the Rust-only rule (2026-10-20T20:45+02:00)

`galion-console-static gen-routes <console-ide 2169d1c>/src/routeTree.gen.ts`
(pure Rust) printed the same 16 `page` lines as the earlier generated file;
`deploy/console-static/routes.2169d1c.txt` is now that output. No Node, npm
or Vite was run after 20:41 +02:00.
