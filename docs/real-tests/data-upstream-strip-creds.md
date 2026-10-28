<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# data-upstream-strip-creds (wave 4 slot 365, F-351-1)

Branch `swarm/data-upstream-strip-creds` off `swarm/console-dist-arena-bank` 7888d8fb.
Box only, VPS read-only, no deploy. Ports 22350-22359 (+ 22068/22069 for the
existing live read-only leg). No new dependencies, `Cargo.lock` unchanged.

## Problem (F-351-1, slot 351)

`--data-upstream` used the same request-head builder as the `--upstream` API
proxy, so every end-to-end header went to the data reader: the browser's
`Cookie` (the paired phone's `galion_admin` token), `Authorization`, any
`X-Api-Key`-style header. The reader's `Set-Cookie` also went back to the
browser, so a reader could set or overwrite `galion_admin` on the console origin.

## Fix (`crates/console-static/src/proxy.rs`, `server.rs`)

- New `proxy::Mode { Api, Data }`. `forward`/`upstream_head`/
  `rewrite_response_head` keep their old behaviour (`Mode::Api`); new
  `forward_mode`/`upstream_head_mode`/`rewrite_response_head_mode`.
- `Mode::Data` request heads: **allowlist** `DATA_FORWARD` = Host, Accept,
  Accept-Language, User-Agent, If-None-Match, If-Modified-Since, Cache-Control.
  Everything else (Cookie, Authorization, Proxy-Authorization, unknown key
  headers, Content-Length, ...) is dropped. X-Forwarded-For/-Proto/-Host are
  rebuilt by the console as before.
- `Mode::Data` response heads: `Set-Cookie` dropped.
- `server.rs`: the data-route branch calls `forward_mode(.., Mode::Data)`.
  The `--upstream` API proxy is unchanged (the console API needs the cookie).
- `--help` text documents the allowlist.

## Tests (times CET +01:00, 2026-10-28)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -D warnings` | ok |
| console-static lib | 59/0 (57 base + `data_mode_strips_credentials`, `data_mode_drops_set_cookie`) |
| workspace test (excl. openmind-sandbox, cgroup-detect; hermes-tools not in this tree) | 143 suites 701/0/7 |
| GALION_REAL `real_data_upstream_creds` (new) | 2/2, x4 runs (05:49-05:50 + full gate run ~05:52) |
| negative control: same test vs pre-fix 7888d8fb release binary (`DATA_CREDS_EXPECT_LEAK=1`) | leaks reproduced in 104 reader heads, reader Set-Cookie reached client |
| GALION_REAL regression `real_dist_arena_bank` (slot 336) | 3/3 |
| live read-only leg `real_dist_arena_live_readonly` (06:00:47-06:00:55) | 1/1 |
| x86_64 musl release `galion-console-static` | 1033088 B |
| aarch64 musl release (rust-lld) | 874632 B |
| `cargo check` aarch64-linux-android, armv7-linux-androideabi, armv7-unknown-linux-musleabihf | ok |
| `scripts/no-python.sh` | ok |
| repo-check | 863 files, 0 errors, 50 warnings (pre-existing) -> PASS |

### Real test `crates/console-static/tests/real_data_upstream_creds.rs`

Real release binary in the live `--dist` shape (`--ui rust --dist DIR
--admin-store FILE --data-upstream 127.0.0.1:22351`), capturing reader on
22351 that records full request heads and answers 200 with
`Set-Cookie: galion_admin=forgedByReader`. Fixture tokens only.

1. Loopback `GET /api/v1/bank?fresh=1` with Cookie (x2), Authorization,
   Proxy-Authorization, X-Api-Key + Accept/User-Agent/If-None-Match: 200,
   body byte-identical; reader head keeps the allowlisted headers and
   `X-Forwarded-For: 127.0.0.1`, has no credential header or value; no
   `Set-Cookie` to the client.
2. `HEAD` same: 200, reader clean.
3. Real pairing `POST /api/v1/admin` on `console.galion.app` -> 43-char admin
   cookie (value never printed). Unpaired remote GET: 401, reader saw nothing.
   Paired GET + bearer: 200, reader head has no token, keeps
   `Host: console.galion.app`.
4. Rust `/bank` page on `console.galion.app` with the paired cookie: 200,
   14920 B, "OpenRouter" + "$9.50" rendered; console-ui's self-upstream fetch
   reached the reader once, without cookie/token.
5. 100 credentialed GETs (half paired-phone, half loopback): 12-16 ms total,
   0 reader heads with credentials.
6. `real_api_upstream_still_gets_cookie`: `--root --upstream` API proxy still
   forwards Cookie + Authorization and passes Set-Cookie (unchanged).

Output (fixed binary):

```
reader head for loopback GET: leaks []
loopback GET /api/v1/bank -> 200, client Set-Cookie: false
reader head for loopback HEAD: leaks []
paired: admin cookie len 43 (value not printed)
reader head for paired phone GET: leaks []
reader head for /bank render #0: leaks []
/bank render -> 200 14920 bytes, reader requests 1
100 credentialed GETs in 16 ms, reader heads with credentials: 0
--upstream API proxy: Cookie + Authorization forwarded, Set-Cookie passed (unchanged)
```

Negative control (pre-fix 7888d8fb, 05:50:14):

```
reader head for loopback GET: leaks ["bearer", "api-key", "cookie-value", "Cookie header", "Authorization header", "galion_admin"]
loopback GET /api/v1/bank -> 200, client Set-Cookie: true
reader head for paired phone GET: leaks ["bearer", "Cookie header", "Authorization header", "galion_admin"]
reader head for /bank render #0: leaks ["Cookie header", "galion_admin"]
100 credentialed GETs in 15 ms, reader heads with credentials: 100
NEGATIVE CONTROL: old binary leaked in 104 reader heads (F-351-1 reproduced)
```

Commands:

```
GALION_REAL=1 cargo test --release -p console-static --test real_data_upstream_creds -- --nocapture --test-threads=1
GALION_REAL=1 DATA_CREDS_EXPECT_LEAK=1 DATA_CREDS_BIN=/path/to/7888d8fb/galion-console-static \
  cargo test --release -p console-static --test real_data_upstream_creds -- --nocapture --test-threads=1
```

## Live VPS (read-only, 2026-10-28 05:50 CET)

- `console.service` (:8080, MainPID 240630, NRestarts 0) and
  `console-rust-primary.service` (:18080, MainPID 240579, NRestarts 0) both run
  `galion-console-serve-2d846fb8 --dist /opt/galion/console-dist --ui rust
  --local-api --pulse 127.0.0.1:8091 --desk 127.0.0.1:8646` - **no
  `--data-upstream`**, so F-351-1 is not exposed live today; the fix lands
  before the bank reader is wired (D-351-1).
- `/api/v1/bank` on :8080 and :18080: 404.
- Nerve 127.0.0.1:8670 /state: cycle 55, console healthy (pid 240630, rss 724 kB,
  restarts_1h 0). Cortex: not checked as a live source (nerve used).
- Live leg 06:00:47-06:00:55 through `ssh -L 22069 -> :18080` (GETs only): live
  board 12 lines -> 12/12 ids rendered; `/api/v1/bank` via the new Data-mode
  proxy = live 404, `/bank` honest empty state.

## Owner decisions

- **D-365-1** Land this before any `--data-upstream` wiring (bank reader
  D-351-1 / D-336-2). Fold order: on top of 7888d8fb (and slot 351 65fa0271
  docs, which reference F-351-1).
- **D-365-2** Allowlist content: is `Accept-Encoding` wanted (reader could
  gzip)? Left out on purpose (console-ui expects identity bodies).
- **D-365-3** If a future reader needs auth, give it its own loopback secret
  in its unit (console stays key-free) rather than forwarding the browser's.

No deploy, no live change, no main/main-staging push.
