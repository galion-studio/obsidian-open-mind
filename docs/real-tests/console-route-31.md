<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: console-route-31 (`/hermes` Rust-first under an SPA fallback) vs the live VPS, read-only

Slot 170, box ports 20400-20409. `crates/console-static/tests/real_route31.rs` (`#[ignore]` + `GALION_REAL=1`): one
`ssh -N` forward (alias `slavik-vps`, address not in the code) 20401 -> VPS 127.0.0.1:8080 (console API), 20403 -> 8670
(galion-nerve); the real release `galion-console-static --ui rust --upstream 127.0.0.1:20401 --root <tmp SPA root>
--routes <31 live page routes> --proxy-prefix /api/ --bind 127.0.0.1:20400`. **Only GETs reach the VPS**; the
POST/PUT/DELETE probes are answered 405 by the binary on the box and never forwarded. Nothing was started, written or
changed on the VPS.

```
GALION_REAL=1 cargo-slot test -j 2 --release -p console-static --test real_route31 -- --ignored --nocapture
```

## Run 2026-10-21 10:14:54-10:15:25 CEST (+02:00)

```
real: forward 127.0.0.1:20401->8080 127.0.0.1:20403->8670 (ssh -N, GET only)
real: nerve before: cycle 63 mode live components 15 -> ["hermes-rs:8642 degrading", "console:8080 healthy"]
real: binary on 127.0.0.1:20400 with --root (SPA fallback) + 31 page routes
real: /hermes 200 28157 B in 847 ms
real: /hermes threads 214 (listed 60), first thread items 18, CSP unchanged, no script
real: POST/PUT/DELETE /hermes -> 405 Allow: GET, HEAD (in the binary, nothing sent)
real: Rust-first 18 -> ["/agents 200", "/chill 200", "/cloudflare 200", "/console 200", "/context 200", "/data 200", "/docs 200", "/domains 200", "/gateway 200", "/guardian 200", "/hermes 200", "/ 302", "/jupyter 200", "/pack 200", "/repos 200", "/services 200", "/soon 200", "/status 200"]
real: SPA fallback 13 -> ["/analytics", "/arena", "/bank", "/cache", "/instances", "/keys", "/markers", "/md/user", "/memory", "/mesh", "/node", "/security", "/session"]
real: proxy GET /api/hermes/health -> 200 (direct 200)
real: stopped galion-console-static
real: nerve after: cycle 63 mode live components 15 -> ["hermes-rs:8642 degrading", "console:8080 healthy"]
real: stopped ssh forward
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.11s
```

Checked against live data: the Rust `/hermes` thread count equals live `/api/hermes/history` (214, 60 links); the first
thread's item count (18) equals live `history?id=`; the bind address is not in the page; CSP and X-Frame-Options exact.

Nerve (VPS 127.0.0.1:8670 `/state`, openmind-nerve aa77864, mode live): `hermes-rs:8642 degrading` before and after is the
open incident "hermes-rs: degrading: 1 restart(s) in the last hour", opened 09:46:24 +02:00 by the slot-180 binary switch
(expected to clear about 1 h after it), not by this test. console:8080 healthy before and after. The cortex is not live
on the VPS yet; the nerve was used.

Regression, same code: `real_port2` (slot 136 test, ports 20060-20063) 10:15:31-10:16:07 +02:00: 1 passed, `/hermes`
200 28157 B, 214 threads (60 listed), thread items 18; `/context`, 9 `/gateway` GETs, `/guardian` 401 as before.
After both runs: no listener left on 20400-20409 or 20060-20069.
