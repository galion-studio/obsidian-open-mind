# PROGRESS — OpenMind foundation

Append-only. Newest at bottom. Times: Europe/Zagreb RFC 3339.

- 2026-10-18T18:59:17+02:00 | setup | teacher | git init, git+ripgrep installed, teacher pack written
- 2026-10-18T19:35:36+02:00 | T1 | S1 | PASS 8/10 | C1 ok C2 ok C3 ok C7 ok C8 ok (crates/ scope) | workspace root Cargo.toml resolver=2, opt-level="z" (I/O helpers, footprint), lto/cgu=1/panic=abort/strip; LICENSE in root + every crate; openmind-schema copied out with workspace fields; rustfmt+clippy components were missing, S1 added them; DEDUCTIONS: S1 committed itself before lead review and its `git add` swept in contrib/grokbot/ + release/ (another team's area) and git stamped both commits +0000 (server time, not Zagreb) — history not rewritten, noted per teacher
- 2026-10-18T19:35:36+02:00 | T2 | S1 | PASS 9/10 | C1 ok C2 ok C3 ok C6 ok C7 ok C8 ok | chrono+chrono-tz (ticket's allowed alternative to jiff, tz data compiled in so tests are offline); 8 tests pass incl. DST 2026-10-25T00:59:59Z->02:59:59+02:00 and 01:00:00Z->02:00:00+01:00; output never ends in Z, never +00:00; re-exported as openmind_schema::time
- 2026-10-18T19:35:36+02:00 | T3 | S2 | PASS 8/10 | C1 ok C2 ok C3 ok C4 ok C5 ok C7 ok C8 ok-on-its-own-scope | galion-one 615,624 B (was 1,060,288 not stripped), galion-desk 402,720 B (was 4,735,088), room 344,304 B (was 4,601,224), all < 2 MB and `file` says stripped; libsqlite3 kept via raw FFI (ldd shows libsqlite3.so.0); every read-only route identical in the child run; originals untouched (mtimes unchanged); DEDUCTIONS: C8 as written fails for the workspace (runtime python3 spawns still in the ported code) -> deferred to T3b with teacher
- 2026-10-18T19:35:36+02:00 | T4 | S3 | PASS 9/10 | C1 ok C2 ok C3 ok C4 ok C5b ok C6 ok C7 ok C8 ok | desk-hermes 1,427,792 B and link-watch 394,392 B, both stripped; rusqlite opened SQLITE_OPEN_READ_ONLY, read a `cp` copy of state.db, never the live file; link-watch golden diff empty under a `sed` normaliser of the documented timestamp/ms fields (jq absent); new Zagreb field `at_local` = 2026-10-18T19:25:56+02:00 while UTC was 17:25:56Z; desk-hermes Python reference renamed to comments only; DEDUCTION: desk-hermes has no byte-golden because the old stdout is not persisted anywhere on this box and running the Python is forbidden by the rules (key order + two-run + shell-oracle evidence used instead)

## Lead-run checks (CHECKS.md) — 2026-10-18T19:35:36+02:00, cd /opt/galion/openmind, PATH=/root/.cargo/bin
- C1 `cargo fmt --all -- --check` exit 0
- C2 `nice -n 19 cargo clippy -j 2 --workspace --all-targets -- -D warnings` exit 0
- C3 `nice -n 19 cargo test -j 2 --workspace` exit 0 (openmind-schema 1 passed, zagreb-time 8 passed, bins 0) and `nice -n 19 cargo build -j 2 --release --workspace` exit 0
- C4 five bins < 2 MB, all `file` = stripped (615,624 / 402,720 / 344,304 / 1,427,792 / 394,392)
- C5 re-run by the lead: new desk on DESK_BIND=127.0.0.1:18646 vs live 8646 — GET /health, GET /desk and GET / IDENTICAL (diff exit 0, Date stripped), live-vs-live baseline identical, port 18646 released after kill
- C6 `cargo test -p zagreb-time` 8 passed incl. the 2026-10-25 switch; UTC scan hits only ported originals + link-watch's compatibility `at` field
- C7 CJK scan: no matches (exit 1)
- C8 `find` = no .py files; `rg -i python crates/` hits: galion-desk:236 and galion-one:1136,1238,1830 (verbatim live code, T3b scope, NOT comments) + comment-only hits in desk-hermes/link-watch -> workspace-wide C8 FAILS until T3b
- C9 hermes, console, galion-desk, link-watch all active, 127.0.0.1:8091/health = 200, 8002 = 503 (its own state), no leftover lead test processes, ports 18091/18002/18646/14020 free
- C10 every build ran under `nice -n 19` with `-j 2`; 3 children max alive
- Note (not ours, not touched): pid 1013024 is a long-lived foreign `bash -c` from another session's release work that rewrites /opt/galion/galion-light/one/one and states it writes Hermes state.db. Left alone.

## Status 2026-10-18T19:35:36+02:00
- Passed: T1, T2, T3 (own scope), T4 — graded above. Workspace WIDE C8 still fails until T3b.
- Not committed by the lead yet at this stamp: crates/{galion-one,galion-desk,room,desk-hermes,link-watch}, Cargo.lock, TEACHER/TICKETS.md (T3b ticket) — see the commit that adds this line.
- Running now (this turn, after this line): S3b = T3b (remove runtime Python), S6 = T6 (console production build in a copy, staged). T5 (galion-one systemd unit) goes out next turn so dist/bin/galion-one is cut from the post-T3b binary, not from source that T3b is still editing.
- Next step: lead grades T3b and T6, then dispatches T5, then hands the STAGED.md list to Slavik.
- Commit hygiene from now on: only the lead commits and only after a PASS; `git add` with explicit paths only (Cargo.toml Cargo.lock crates/ deploy/ TEACHER/ README.md LICENSE .gitignore); never release/ contrib/ docs/ personas/ stage/ admin.preferences*; commit message carries a Zagreb stamp.
