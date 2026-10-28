<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: tuner-cutover (swarm slot 07, ports 19070-19079)

All times are Europe/Zagreb, CEST (+02:00), 2026-10-20. Box: the shared swarm box. VPS: `slavik-vps`, read-only only. The VPS probes were `systemctl status/cat/show`, `ss -ltnp`, `journalctl` reads, `ls`/`sha256sum`/`cat` of `/opt/galion/tuner/**` and `galion-deploy`, and HTTP GETs to `127.0.0.1:8650`, `:8660/status.json` and `:8670/`. Nothing was written, copied, restarted or POSTed there. No secrets appear below.

## 1. Live identification (18:51-19:08)

- `tuner.service`: active since 2026-10-18 19:18:07 (+02:00), pid 1196273, `ExecStart=/opt/galion/tuner/bin/tuner`, env `TUNER_ADDR=127.0.0.1:8650`, `TUNER_ROOT=/opt/galion/tuner/www`, `Restart=always`, `RestartSec=2`, `Nice=10`, `NoNewPrivileges=true`, no drop-ins.
- Binary sha256 `ecad72a4368a3c9155a676b772ffb7c0caf863884c8614d9fdfcb5604866dd25`, 508,680 B, static-pie x86-64, built from `/opt/galion/tuner/src/main.rs` (sha256 `c5d8d85ab84357b304ba504163a0bc996765704992726457a11be85a80c39587`). The live source was read. Its logic matches `crates/tuner` (same whitelist, headers, 405/404/500 paths, 4096-byte single read, 5 s read timeout, one thread per connection).
- www sha256s are identical to `crates/tuner/www`: app.js `6c93cb2d…`, index.html `aea57c1c…`, pitch.js `ccbc015c…`, style.css `65aa0961…`. `test/test.js` is identical too.
- `ss -ltnp`: `127.0.0.1:8650 users:(("tuner",pid=1196273,fd=3))`. There were 0 established connections at 19:08.
- `tuner-quick.service` is transient: `cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650`. galion-health 8660: `"tuner","unit":"tuner.service","unit_state":"active","port":8650,"http":"200","ok":true`.

## 2. Box vs live route diff: `scripts/real/tuner-cutover.sh`

Release `x86_64-unknown-linux-musl` binary (sha256 `47a17df2…1370`) on `127.0.0.1:19070`, root `crates/tuner/www`. The same 21 raw GET requests went to the box and, over ssh, to VPS `127.0.0.1:8650`, and were compared with `cmp` (whole response: status, headers, body).

Final run, 19:10:55-19:10:59:

```
route                status                         box    live result
root                 200 OK                        2700    2700 IDENTICAL
index                200 OK                        2700    2700 IDENTICAL
app-js               200 OK                        4920    4920 IDENTICAL
pitch-js             200 OK                        4616    4616 IDENTICAL
style-css            200 OK                        3080    3080 IDENTICAL
health               200 OK                         335     335 IDENTICAL
health-query         200 OK                         335     335 IDENTICAL
root-query           200 OK                        2700    2700 IDENTICAL
index-query          200 OK                        2700    2700 IDENTICAL
nope                 404 Not Found                  334     334 IDENTICAL
health-upper         404 Not Found                  334     334 IDENTICAL
double-slash         404 Not Found                  334     334 IDENTICAL
app-js-slash         404 Not Found                  334     334 IDENTICAL
traversal            404 Not Found                  334     334 IDENTICAL
encoded-traversal    404 Not Found                  334     334 IDENTICAL
test-js              404 Not Found                  334     334 IDENTICAL
src-main             404 Not Found                  334     334 IDENTICAL
favicon              404 Not Found                  334     334 IDENTICAL
http10               200 OK                         335     335 IDENTICAL
bare-get             200 OK                        2700    2700 IDENTICAL
no-version           200 OK                         335     335 IDENTICAL
[2026-10-20T19:10:59+02:00] routes: 21, identical: 21, diff: 0 (against live VPS 8650)
```

Earlier runs: 19:06:06 (fixture capture) and 19:07:47 (21/21 IDENTICAL). One run at 19:10:38 was ended by an outside SIGTERM (exit 143) during the ssh capture, before any comparison. The re-run 17 s later passed, as above. On the VPS side, `cat` reported "Connection reset by peer" once or twice per capture. The captured bytes were still complete and identical across runs. It happens with the old live binary, not on the box.

Box process during the run: `pid 856874, RSS 324 KB` (live: 348 KB). Port 19070 was closed after the run, and no tuner processes were left.

## 3. Cargo real tests (`GALION_REAL=1`, `crates/tuner/tests/real_tuner.rs`, ports 19071-19073)

`GALION_REAL=1 TUNER_REAL_BIN=/workspace/swarm/artifacts/tuner/tuner-x86_64-unknown-linux-musl cargo test -p tuner --test real_tuner`, re-run verbose at 19:11:34 (the script run at 19:10:59 gave the same 3 passed):

```
test real_busy_port_exits_1_cleanly ... ok
test real_flags_exit_without_binding ... ok
test real_binary_matches_live_on_every_route ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```

What they cover: `matches_live_on_every_route` sends the 21 routes over TCP and compares them with the live fixtures, plus HEAD, POST (405), 16 parallel clients, and an idle client that must not block others. `busy_port` checks `tuner: bind 127.0.0.1:19072: ...` with exit 1 and no panic. `flags` checks that `--version` exits 0 and 19073 stays free, `--check` exits 0 or 1, and an unknown argument exits 2.

## 4. aarch64 musl under qemu (19:08:19)

`qemu-aarch64-static tuner-aarch64-unknown-linux-musl --version` gave `tuner 0.1.0`. Served on `127.0.0.1:19074`, the 21 routes vs the live fixtures gave **21 routes, 0 diffs**. Killed afterwards, and 19074 was closed.

## 5. Restart gap (19:09)

Stop then start of the x86_64 musl binary on `127.0.0.1:19075`, 5 runs: port closed for 3, 4, 3, 3, 3 ms, `/health` 200 after each. Basis for the downtime estimate in `docs/cutover/tuner.md`.

## 6. Reproducibility

The same tree was copied to another path and built with another `CARGO_TARGET_DIR`. The sha256s matched for both artefacts (`47a17df2…`, `0aca8af6…`).

## 7. Gates

- `cargo fmt --check`: ok
- `cargo clippy --workspace --all-targets -- -D warnings`: ok
- `cargo test --workspace`: ok, 506 passed, 0 failed (123 test binaries)
- `cargo check -p tuner --all-targets` for `armv7-unknown-linux-musleabihf` and `aarch64-linux-android`: ok
- `scripts/no-python.sh`: ok

> **2026-10-21 (realscript-port-3):** the shell driver for this evidence was ported to Rust and deleted; re-run with the matching test in `crates/galion-realtest/tests/` (see `docs/real-tests/realscript-port-3.md`).
