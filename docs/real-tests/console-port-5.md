<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: console-port-5 rooms against the live VPS (read-only)

Slot 139 ports 20090-20099 on the box. `crates/console-static/tests/real_port5.rs` (`#[ignore]` + `GALION_REAL=1`):
opens `ssh -N` to the VPS (alias `slavik-vps`, address not in the code) with forwards 20091 -> 127.0.0.1:8080 (console
API) and 20093 -> 8670 (galion-nerve), starts the real release `galion-console-static --ui rust --upstream
127.0.0.1:20091 --bind 127.0.0.1:20090`, compares every room with the live API read through the same forward, then
reads the live build-session folder over ssh (`stat -c '%n %s'` and `cat` of the first file: read-only, in memory, not
printed or committed), serves it on 20092 in the `/api/v1/session` shape and starts a second binary on 20094 against
it, to check the paired view with real data. Then it stops both binaries and the forward. **Only GETs reach the VPS
console.** Scan and Heal are tested against a fake upstream only (`tests/ui_port5.rs`). Nothing was started, written or
changed on the VPS.

```
GALION_REAL=1 cargo test --release -p console-static --test real_port5 -- --ignored --nocapture
```

## Runs, 2026-10-21 (+02:00), branch code as committed
05:47:03-05:47:35, 05:49:40-05:50:11, 05:50:11-05:50:34 CEST: 3 of 3 passed (22.3-22.9 s each), identical results.
Third run (IP addresses replaced by `<ip>`):

```
real: forward 127.0.0.1:20091->8080 127.0.0.1:20093->8670 (ssh -N, GET only)
real: nerve before: cycle 6631 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /node 200 15613 B in 844 ms
real: /node ok: ssh line and Last IP equal live health target (pod ip50, sshPort 22)
real: /security 200 16280 B in 844 ms
real: /security /api/v1/security live 401 -> page states it
real: /security /api/v3/guardian/status live 401 -> page states it
real: /security /api/v3/guardian/incidents live 401 -> page states it
real: /session live list 401 -> page states it
real: /session?name=00-summary.md live 401 -> page states it
real: /api/v1/session-pack via the Rust front 401 = live 401
real: /session paired view with live folder data: 30 files listed (names+sizes equal), first file 1360 B rendered whole in 0 ms
real: nerve after: cycle 6631 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static (real session data)
real: stopped galion-console-static (live upstream)
real: stopped ssh forward
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 22.92s
```

What is checked against live data (not only status codes):
- `/node`: the page's SSH line equals `ssh <target.sshDirect> -p <target.sshPort> -i ~/.ssh/id_ed25519` built from the
  live `/api/v1/health` target, and "Last IP" equals live `target.publicIp` (the same values the TSX had compiled in
  from `machine.ts`).
- `/security`: the three admin-only sources answer `401 {"error":"phone required"}` to an unpaired caller (the box has
  no phone cookie, on purpose); the page states each refusal exactly ("Security did not load: HTTP 401 (phone
  required).", same for Guardian status and Incidents) and shows "—" for Denylist instead of the TSX default "on"; no
  "None open." is claimed.
- `/session`: list and file answer 401 unpaired and the page states it; a `../` name answers 400 without asking;
  `/api/v1/session-pack` through the Rust front's `/api/` proxy gives the same status as a direct GET (401).
- Paired view with real data: the live folder has 30 `.md` files, all matching `safeName`; the page lists 30 links
  with names and sizes equal to `stat`; the first file (1 360 B, size from `stat` = bytes read) is rendered whole and
  escaped inside `<pre>`.
- No `<script>` in any room; the nerve (VPS 127.0.0.1:8670 `/state`, mode live, 15 components) saw console, desk,
  hermes-rs and galion-one healthy before and after. The cortex is not live: galion-nerve-shadow (8672, observe-only)
  answers `/healthz` 200 `ok`; it was only read.
- After the runs: no listener left on 20090-20099; VPS `console.service` ActiveState=active, NRestarts=0, 0
  error/fail journal lines in the last 15 min (read 05:48 CEST).

## Live API shapes (read 2026-10-21 05:40 CEST, values not committed)
`/api/v1/health`: {status, service, version 3.8.3-board, project, phi, mode, zones, target{provider, profile, mode, pod,
podId, sshUser, sshHost, sshDirect, sshPort, publicIp, gpu, httpPort, originPort, connected}, github, privateVault, note}.
`/api/v1/security`, `/api/v1/session`, `/api/v1/session?name=`, `/api/v1/session-pack`, `/api/v3/guardian/status`,
`/api/v3/guardian/incidents`: 401 `{"error":"phone required"}` unpaired. Paired shapes from the TSX server code
(`routes/api/v1/{security,session}.ts`, `lib/session-docs.ts`) are used in the fakes.

## Box checks (05:44-05:50 +02:00)
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude
  cgroup-detect -- -D warnings` ok; `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 549
  passed, 0 failed, 2 ignored (125 suites).
- console-ui lib 35 tests (9 new); console-static: ui.rs 11, ui_port5.rs 9, unit 23, serve 6.
- `galion-console-static` release: x86_64 musl 770 848 B (static-pie), aarch64 musl 671 456 B (rust-lld); armv7
  musleabihf and aarch64-linux-android `cargo check` ok. `scripts/no-python.sh` ok. repo-check: 781 files, 0 errors,
  48 warnings -> PASS (43 pre-existing; 5 new are UTC `…Z` inputs in the Guardian API's own format that the page
  parses and shows on the Zagreb clock). No new crates (`Cargo.lock` unchanged).
