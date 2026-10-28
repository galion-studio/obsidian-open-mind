<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-session-pack (wave 4 slot 265)

Branch `swarm/console-session-pack`, based on `swarm/console-v2-audit` 2a012311
(console-port-merge). Box only, Europe/Zagreb (CET +01:00), 2026-10-27.
**No VPS writes.** Live console cutover (console-dist/srvx) was not touched.

## What changed
1. **Pairing path (console-ui)** — after a successful code pair, the one-time
   `device` secret from `POST /api/v1/admin` is stored as an HttpOnly
   `galion_device` cookie (Secure on galion.app / forwarded https). When a
   console host presents that cookie but `you` is false (`paired:true` store,
   cookie missing/expired), the gate resumes with `POST {device}` (SPA used
   `localStorage` + JS). A refused device cookie is cleared so the 5-fail lock
   is not burned on every page load. Form posts may also send `device=`.
2. **`/api/v1/session-pack` (console-static proxy)** — refuses GET/HEAD without
   a non-empty `galion_admin` cookie (`401 {"error":"phone required"}`) before
   opening upstream; other methods 405. On 200, forces
   `Content-Type: application/zip`,
   `Content-Disposition: attachment; filename="build-session.zip"`,
   `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`.

## Ports
21320 front, 21321 fake upstream (box tests). Range 21320-21329 reserved.

## Commands and evidence
See CALL-RECORD DONE line and the cargo test log under
`/workspace/swarm/target/console-session-pack/`. Unit + UI tests cover device
resume, cookie Secure flag, unpaired pack 401, paired pack headers.

## Box probe 2026-10-27 17:39:23 +0100 (CET +01:00)
```
# unpaired pack
HTTP/1.1 401 Unauthorized
Content-Type: application/json
Cache-Control: no-store
X-Content-Type-Options: nosniff
Content-Length: 26
Connection: close

BODY: {"error":"phone required"}

# paired pack headers
HTTP/1.1 200 OK
Content-Length: 11
Connection: close
Content-Type: application/zip
Content-Disposition: attachment; filename="build-session.zip"
Cache-Control: no-store
X-Content-Type-Options: nosniff
Connection: close

BODY_PREFIX:  50 4b 03 04 5a 49 50 46

# device resume
HTTP/1.1 303 Unknown
Location: /agents
Content-Type: text/plain; charset=utf-8
Cache-Control: no-store
Set-Cookie: galion_admin=good; HttpOnly; Path=/; SameSite=Lax
X-Content-Type-Options: nosniff
Referrer-Policy: strict-origin-when-cross-origin
Content-Length: 0
Connection: keep-alive


# pair form sets device cookie
HTTP/1.1 303 Unknown
Location: /session
Content-Type: text/plain; charset=utf-8
Cache-Control: no-store
Set-Cookie: galion_admin=good; HttpOnly; Path=/; SameSite=Lax
Set-Cookie: galion_device=devSecretToken01ab; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000; Secure
X-Content-Type-Options: nosniff
Referrer-Policy: strict-origin-when-cross-origin
Content-Length: 0
Connection: close

```
