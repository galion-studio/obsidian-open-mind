<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: `swarm/cortex-s2-c` (cortex S2 T2.8 `proc-owner`, T2.9 `surface`)

- Written: 2026-10-20 23:30 CEST (+02:00). Slot 57, ports 19570-19579. Base `swarm/cortex-sensor` e536446.
- Code: `crates/openmind-nerve/src/sensors/proc_owner.rs`, `src/sensors/surface.rs`, 2+2 lines in `sensors/mod.rs`.
  Tests: unit tests in both files, `tests/cortex_s2_c.rs` (fixtures + replay), `tests/real_cortex_s2_c.rs` (GALION_REAL=1).

## Configuration (both sensors are off unless configured)

| sensor | enable | other settings |
|---|---|---|
| `proc-owner` | `NERVE_PROC_OWNER_USERS=root,galion` (names or uids) | `NERVE_PROC_OWNER_PORTS=8420=galion,...` per-port expected user; `NERVE_PROC_OWNER_PROC`, `NERVE_PROC_OWNER_PASSWD` (fixture roots) |
| `surface` | `NERVE_SURFACE_TARGETS=https://galion.app,http://127.0.0.1:8420/health@200` | `NERVE_SURFACE_EVERY_S` 300, `NERVE_SURFACE_TIMEOUT_MS` 3000, `NERVE_SURFACE_WARN_DAYS` 7, `NERVE_SURFACE_CRIT_DAYS` 2 |

Both do nothing when `Input.live` is false (replay, tests). `NERVE_LISTEN_SCOPE` limits `proc-owner`.
Findings are aggregated per check (one deduction for 30 strays, not 30). Titles and B33 event `data` carry ports,
uids, target numbers (`#3`), counts, days and status codes only; user names, pids, exe paths, hostnames and error texts
stay in loopback-only `evidence`.

| id | kind / sev | meaning |
|---|---|---|
| `stray-owner` | vulnerability / warn (error if any is not loopback) | listener owned by a user outside the policy |
| `deleted-binary` | vulnerability / error | listener's process runs an exe that is `(deleted)` |
| `proc-owner-unresolved` | vulnerability / info | socket with no readable owning process (nerve not root) |
| `surface-dns` | bleeding / error, cause `edge` | target does not resolve |
| `surface-dns-proxied` | bleeding / info | every address in 198.18.0.0/15 (fake-IP resolver): DNS verdict not authoritative |
| `surface-tls-expired` / `-expiring` / `-fail` | bleeding / crit / warn or error / error | leaf `notAfter` passed / within WARN (CRIT) days / handshake failed |
| `surface-tls-unread` | bleeding / info | server refuses TLS 1.2 (TLS-1.3-only): expiry not readable without a TLS stack |
| `surface-http-down` / `-status` | bleeding / error / warn (error on 5xx) | no HTTP answer / status != `@NNN` (default: >= 400) |

## 1. Gates (box, 2026-10-20 23:23-23:28 CEST +02:00)

```
cargo fmt --all --check                                   -> ok
cargo clippy -j 2 --workspace --all-targets -- -D warnings -> ok
cargo test -j 2 --workspace                               -> 980 passed, 0 failed
cargo check -p openmind-nerve --target {x86_64,aarch64}-unknown-linux-musl, armv7-unknown-linux-musleabihf, aarch64-linux-android -> ok
scripts/no-python.sh -> no-python: ok ; scripts/spdx-scan.sh -> scanned 364, missing 0
```

## 2. GALION_REAL=1 real binary on the box (2026-10-20 23:22:45 -> 23:23:02 CEST +02:00)

`GALION_REAL=1 nice -n 10 cargo test -j 2 --release -p openmind-nerve --test real_cortex_s2_c -- --nocapture`

Setup inside the Rust test: release `openmind-nerve serve` on 127.0.0.1:19570 (`NERVE_PROC_OWNER_USERS=root`,
ports 19570/19573/19574 may be the box user); stray listener :19571 (test process); a copy of the nerve binary on
:19572, file deleted after start; fake TLS 1.2 server :19573 replaying the real galion.app flight (and dropping a
ClientHello without SNI); HTTP origin :19574 answering 503 for `@200`; nothing on :19575. Output (verbatim):

```
event {"ts":"2026-10-20T23:22:57+02:00","source":"proc-owner","kind":"vulnerability","severity":"high","data":{"findings":2,"ids":["deleted-binary","stray-owner"],"top":{"cause":null,"component":"host","evidence_n":1,"fix":"the process runs code no longer on disk (old build, unpatched, unauditable): restart its unit onto the installed binary (owner approval)","id":"deleted-binary","title":"1 listener(s) run a deleted binary: :19572"}}}
event {"ts":"2026-10-20T23:22:57+02:00","source":"surface","kind":"bleeding","severity":"high","data":{"findings":3,"ids":["surface-http-down","surface-http-status","surface-tls-expiring"],"top":{"cause":"edge","component":"edge","evidence_n":1,"fix":"check the tunnel ingress rule and the origin service behind it","id":"surface-http-down","title":"1 of 3 public target(s) do not answer HTTP: #3"}}}
serve :19570 cycle 2: cycle_ms 29, rss 3700 KiB, sensors us [listeners=10 scanners=1 edge=0 components=0 ddos=11 proc-owner=4465 surface=42]
finding stray-owner [warn] "2 listener(s) run as an unexpected user: :19571 (uid 1000) :19572 (uid 1000)"
finding deleted-binary [error] "1 listener(s) run a deleted binary: :19572"
finding surface-tls-expiring [warn] "1 of 3 public target(s) have a TLS certificate expiring soon: #1 (10 d)"
finding surface-http-status [error] "1 of 3 public target(s) answer an unexpected HTTP status: #2 (503, want 200)"
finding surface-http-down [error] "1 of 3 public target(s) do not answer HTTP: #3"
serve :19570 cycle 3: strays gone, proc-owner findings cleared
test real_proc_owner_and_surface_on_the_box ... ok
```

Events parse as `openmind_schema::NerveEvent`, carry `+02:00`, and contain no `localhost`, `/tmp` or exe names
(asserted). Budget: cycle 29 ms (< 50 ms), RSS 3.7 MiB (< 8 MiB). `proc-owner` cost 4.5 ms in the cycle that
re-scanned `/proc/*/fd` (the listening-inode set had changed); unchanged sets reuse the cache (unit test
`fd_scan_on_a_fixture_proc_tree_is_cached`). `surface` costs ~40 us per cycle: the round runs on its own thread.
All started processes were killed by the test (`ss -ltn | grep :1957` empty afterwards).

## 3. Fixture / replay tests (no network)

- `proc_owner_fixture_matches_golden`: VPS-like `/proc/net/tcp{,6}` + fd map + passwd -> `proc-owner.expected.json`
  (stray :8654/:8655 (uid 1001) and :8664 (root where galion expected), deleted binary :8654, 1 unresolved).
- `real_tls12_flight_fixture_gives_the_real_expiry`: the captured galion.app flight -> notAfter 1793467688
  (2026-10-31 18:28:08+01:00).
- `replay_with_both_sensors_on_keeps_the_golden`: `openmind-nerve replay fixtures/cortex-replay` with both sensors
  configured (one target `.invalid`) equals the pre-trait golden: nothing read, nothing contacted.

## 4. One manual read-only check of real hostnames (box, 2026-10-20 23:18:10 CEST +02:00)

Not a test (tests stay loopback-only): a scratch build outside the repo ran one `surface` round (DNS + TLS 1.2
handshake up to Certificate, or one GET) against 8 hostnames from evidence/tunnel-audit/hosts-owners.txt:

```
#1 https://galion.app          dns=Ok(1) tls=notAfter in 10 d
#2 https://api.galion.app      dns=Ok(1) tls=notAfter in 44 d
#3 https://tuner.galion.app    dns=Ok(1) tls=Failed("read: failed to fill whole buffer")
#4 https://galion.studio       dns=Ok(1) tls=notAfter in 35 d
#5 https://merlinelement.com   dns=Ok(1) tls=notAfter in 54 d
#6 http://workspace3d.app      http=Err("connection timed out") (3003 ms)
#7 http://maivirtue.com        http=Err("connection timed out") (3003 ms)
#8 https://slaviknomad.com     dns=Ok(1) tls=notAfter in 39 d
```

- galion.app leaf: `CN galion.app`, SAN `*.galion.app`, issuer Google Trust Services WE1, notBefore 2026-10-01,
  **notAfter 2026-10-31 18:28:08+01:00 = launch day**. A 30-day edge certificate (Cloudflare Universal SSL normally
  renews it before expiry); with the default 7-day warning the sensor will flag it from Oct 24 if it is not renewed.
- The box resolver is a **fake-IP egress proxy**: every name, even `nonexistent-zz9.galion.app` and
  `tuner.galion.app`, resolves to 198.18.0.1 (`getent ahostsv4`, nameserver 198.18.0.53). Port 80 egress times
  out. So on the box DNS and plain-HTTP verdicts are meaningless: that is why `surface-dns-proxied` exists, and why
  #3/#6/#7 above are box artefacts, not edge faults.
- Read-only VPS probe (`ssh slavik-vps`, 2026-10-20 23:26:42+02:00, `getent ahostsv4`): galion.app and
  api.galion.app -> 188.114.96.11 188.114.97.11 (Cloudflare); **tuner.galion.app -> no address (NXDOMAIN, still
  as in REVIEW.md:100)**. `ss -ltn`: 22 listening sockets. `systemctl show galion-nerve`: User=root, active.

## 5. Not done / open

- HTTP status over TLS (https targets) is not checked: it needs a TLS stack (owner decision D2). https targets get
  DNS + certificate expiry only; give the origin as an `http://127.0.0.1:<port>/...@NNN` target for status.
- No chain/name verification (an expiry check, not a trust check).
- `proc-owner` judges listeners only, not every process (every host has many non-listening root/system processes;
  a process allowlist would be its own policy).
- Not run on the VPS (planning phase: nothing deployed). The VPS socket owner picture needs `ss -ltnpe` or
  `/proc/<pid>/exe` reads, a new command class (owner decision D3).
