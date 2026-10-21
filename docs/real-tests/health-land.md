<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: swarm/health-land (galion-health landed on main)

Nerve plan T1.2(a). Slot 49, ports 19490-19499. All times Europe/Zagreb CEST +02:00.
The VPS was only read: HTTP GET on 127.0.0.1:8660, `systemctl status`/`cat`, `ls -l`, `sha256sum`, `strings | sha256sum`, `ss -ltnp`. Nothing was written, restarted or built there.

## What landed
- `crates/galion-health/` from base.git `ws/foundation` e43d7ab. The logic is unchanged. Changes: SPDX/copyright header, `cargo fmt`, one `type Cache` alias (clippy `type_complexity`), 6 unit tests (`#[cfg(test)]`, not in the release binary), real test `tests/real_health.rs`.
- `deploy/foundation/galion-health.service` from e43d7ab, with only a header added. It matches the live `systemctl cat galion-health` byte for byte on every non-comment line (checked 2026-10-20 22:51 +02:00).
- Not landed, because other agents own them: galion-deploy, galion-backup (+timer/restore test), `deploy/cross-foundation.sh` (it builds health and backup together), journald conf, FOUNDATION-REVIEW/DEPLOY docs.

## Provenance verdict: PROVEN, byte-identical rebuild
| binary | size B | sha256 |
|---|---|---|
| live VPS `/opt/galion/rust/bin/galion-health` (mtime 2026-10-18 22:14 +02:00, unit active since 22:14:38 +02:00) | 503512 | `6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a` |
| box rebuild of e43d7ab tree, its own recipe (`deploy/cross-foundation.sh`: x86_64-unknown-linux-musl, zig 0.13.0 cc, rust-lld, rustc 1.99.1) | 503512 | `6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a` **identical** |
| box rebuild of this branch (same recipe) | 503512 | `6356b6b6694f30316583ab733c0fb999bd3e157cbd896d21555f38f47b93434f` |
| box glibc `cargo build --release` of this branch | 389088 | `700698a5...` (different target, not comparable) |

- `strings -n 6 | sha256sum`: live = e43d7ab rebuild = branch rebuild = `d6f2f3b9...559e99` (1307 lines each).
- Branch vs live: `cmp -l` shows **2 differing bytes**, both inside `.data.rel.ro` at file offsets 481929 and 481953. Read as u32 they are panic `Location` line numbers: 131→222 and 76→135. Line 131 of the e43d7ab main.rs is `String::from_utf8_lossy(&buf[..n])` (a bounds check). The lines moved only because of the header and rustfmt. Code and data are otherwise identical.
- Conclusion: the live galion-health was built from e43d7ab `crates/galion-health` (+ zagreb-time at e43d7ab) with rustc 1.99.1. The build is reproducible, so the VPS binary is no longer the only rollback copy. Re-make it with `git worktree add <dir> e43d7ab` and run `deploy/cross-foundation.sh` without `-p galion-backup`.

## Endpoint diff, live 8660 vs box 19495 (branch musl build, default checks)
Live read 2026-10-20 22:52:05-22:52:18 +02:00, box 22:55:54 +02:00.
| GET | live | box | diff |
|---|---|---|---|
| /status.json | 200 application/json, 1394 B, ok:true 12/12 | 200, 1344 B, ok:false | **same key set** (87 keys, same order; key-sequence sha256 c5a6f4c5... both), same 12 names in the same order. Values differ only because the box has none of the services: `unit_state` "" (no systemd on box), `http` "down" |
| /json | same as /status.json | same | none (alias) |
| /healthz | 200 `ok\n` | 503 `failing: <12 names>\n` | expected (box has no services) |
| / | 200 text/html 1950 B, "12/12 ok" | 200 1898 B, "0/12 ok" | HTML skeleton identical after masking rows/time/count |
| /status | 404 `not found\n` | 404 | none (the endpoint is `/status.json`, there is no `/status`) |
| /nope | 404 | 404 | none |
Headers match on every path (`Content-Type`, `Cache-Control: no-store`, `Connection: close`); only `Content-Length` differs.

Live `/status.json` (22:52:05 +02:00), all 12 ok: openmind-memory 8420 200, hermes-rs 8642 200, link-watch-rs active, galion-desk 8646 200, tuner 8650 200, tuner-quick active, one 8091 200, console 8080 302, cloudflared 20241 200, journald active, ssh 22 tcp-open, galion-backup.timer active.

## Box real test (Rust, gated)
`GALION_REAL=1 cargo test -p galion-health --test real_health -- --ignored --nocapture`. It builds `--release` (or uses `GALION_HEALTH_BIN`), starts the binary on 127.0.0.1:19490 with a fake HTTP upstream on 19491, a closed port 19492 and raw TCP on 19493, checks every endpoint, then kills the processes. Run at 2026-10-20 22:54:59 +02:00: PASS.
```
/status.json -> HTTP/1.1 200 OK
{"ok":true,"checked_at":"2026-10-20T22:54:59+02:00","services":[{"name":"up",...,"port":19491,"http":"200","ms":0,"ok":true},{"name":"raw",...,"http":"tcp-open",...},{"name":"noprobe",...,"port":0,"http":"-",...}]}
/healthz (failing) -> HTTP/1.1 503 Service Unavailable failing: gone
```
No listeners were left on 1949x afterwards (`ss -ltn`).

## Findings (not fixed here, to keep parity with the live binary)
1. **Single `read()` per request.** The server reads once (1024 B) and closes. A client that sends its request in several writes gets a wrong path (falls back to `/`) or a TCP RST, because the close happens with bytes still unread. The first test version hit `ECONNRESET` this way; curl is unaffected. Fix later: read until `\r\n\r\n` or the timeout.
2. **One thread serves all requests and probes on the request path.** With the cache cold, one slow upstream (1.5 s connect + 2.5 s read timeout per check, 12 checks) can block every client for up to about 48 s. The live `ms` values are all 0, so this is not happening today.
3. On a host without systemd, `systemctl is-active` prints nothing on stdout, so `unit_state` is `""` instead of `unknown`. This is cosmetic.
4. `/status` (no `.json`) is 404 on both. Any consumer or plan that says "/status" means `/status.json`.

## Gates and targets (2026-10-20 23:00-23:07 +02:00)
- `cargo fmt --check` OK, `cargo clippy --all-targets -- -D warnings` (whole workspace) OK, `cargo test --workspace` 123 result lines with 0 FAILED, `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` missing 0.
- galion-health: x86_64 musl build 503512 B, aarch64 musl build 446256 B (zig recipe), `cargo check` OK on armv7-unknown-linux-musleabihf and aarch64-linux-android.
