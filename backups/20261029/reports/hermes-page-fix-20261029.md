# Hermes page (console.galion.app/hermes): diagnosis, 2026-10-29 ~08:30 CET

## Verdict
The backend is not down. Nothing was restarted. The page is broken because of a **missing route in the current Rust console build**, and the text box was **removed on purpose** in the Rust cutover.

## Evidence
### Console code (deployed binary galion-console-serve-5aa31927 = commit 5aa31927, 2026-10-28 02:26 CET)
- `crates/console-ui/src/pages/hermes.rs` renders server-side from `GET /api/hermes/health`, `/api/hermes/guard` and `/api/hermes/history`, fetched from the console's own upstream.
- The live `console` unit runs `--dist --ui rust --local-api` with **no `--upstream`**, so it uses itself as the upstream (config.rs: "self-upstream must not proxy").
- `console-static/src/api.rs` only answers health/ping/board/admin/session/arena locally. **No `/api/hermes/*` handler and no proxy to :8642.** The `--data-upstream` default paths are only `/api/arena` and `/api/v1/bank`.
- Those `/api/hermes/*` routes came from the old Node/TSX console (`stack/research-agent/console-ide`, `lib/hermes.ts`). It proxied to 127.0.0.1:8642 with the gateway's API_SERVER_KEY. That was never ported.
- Text box: `hermes.rs` is read-only by design. Its tests assert that there is no `<form`. The page itself says: "Asking, runs, the mic and the guard switches are not on this page yet: they wait on Slavik's JavaScript ruling and the paid-model decision (H1)." The old `<textarea>` exists only in `console-ide/src/components/hermes-panel.tsx` (the Node SPA).

### VPS (read-only checks)
- `hermes-rs.service`: active (running) since 2026-10-27 19:46 CET, NRestarts=0, listening on loopback :8642. 0 error/fail/panic lines in journal for the last 24h.
- No separate "guard" unit exists. Guard is the `/api/hermes/guard` API route, not a service. (`hermes.service` is masked, the legacy one replaced by hermes-rs. `guardian-tick` is unrelated.)
- `curl 127.0.0.1:8642/health` -> 200 `{"status":"ok",...,"implementation":"hermes-rs"}` (~0.5 ms)
- `curl 127.0.0.1:8642/api/hermes/{health,guard,history}` without a key -> 401 gateway_auth_failed (expected: the gateway needs API_SERVER_KEY)
- `curl -H 'Host: 127.0.0.1:8080' 127.0.0.1:8080/api/hermes/{health,guard,history}` -> **404 Not Found** (this is the root cause)
- same Host, `/api/v1/health` -> 200 (console is fine)
- `Host: console.galion.app`, unpaired, `/api/hermes/health` -> 401 (pairing gate, expected; not the cause, since even loopback gets 404)
- `curl 127.0.0.1:8080/hermes` (loopback Host) -> 200: "no answer" x2, "did not load", no form or textarea. This matches what the owner sees.

## What I did
Nothing changed on the server. No restarts: the gateway is healthy and there is no guard unit to restart. console, console-rust-primary, cortex and nerve were not touched.

## Proposed fix (needs owner decision + a code change and deploy; not done)
1. Status lines (gateway/guard/history): add read-only GET proxying of `/api/hermes/{health,guard,history,models,modes,skills}` in console-static to 127.0.0.1:8642. The proxy must add the gateway key server-side (the key is read on the server and never sent to the browser). The pairing gate stays in front. `--data-upstream`/`--data-path` alone is not enough, because the gateway needs the API key header and also answers in a different format (the page expects `{ok,status,ms,bind,loopback,...}` wrappers). So this needs a small adapter in console-static.
2. Text box: only comes back once the owner rules on JS/no-JS for asking (H1 and the JS ruling). Options: a no-JS POST form proxied to the gateway, a small script island, or bringing back the Node SPA panel.
