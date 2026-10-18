# TICKETS — OpenMind foundation (T1–T6)

Repo: /opt/galion/openmind. Rules: LESSON.md §3. Checks: CHECKS.md. Times: Europe/Zagreb RFC 3339.
Dependency order: T1 → (T2, T3, T4 in parallel once the workspace exists) → T5, T6 after T3.

## T1 — Cargo workspace + git + MIT LICENSE + openmind-schema moved in   (S1)
- `cargo new`-style virtual workspace: root `Cargo.toml` with `[workspace] members = ["crates/*"]`, `resolver = "2"`,
  `[workspace.package] edition = "2021", license = "MIT", version = "0.1.0"`.
- `[profile.release]`: `opt-level = "z"` or `3` (document), `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
- Copy (do not move the original!) /opt/galion/openmind-schema → `crates/openmind-schema`; make it use workspace package fields.
- Root `LICENSE` = standard MIT text, line `Copyright (c) 2026 Slavik / Galion Studio`. Same file in each crate dir.
- `.gitignore` with `target/`. `README.md` (short: what, how to build with nice/-j 2, rules).
- git already initialised by teacher; set repo-local `user.name "Hermes (OpenMind)"`, `user.email "hermes@localhost"` and commit.
- Done when: C1–C3, C7, C8 pass; `cargo test -p openmind-schema` passes (existing no-`sk-` test).

## T2 — `zagreb-time` helper crate   (S1)
- Library `crates/zagreb-time`, depending on `jiff` (preferred, with tz database bundled: feature `tzdb-bundle-always` or equivalent) OR `chrono` + `chrono-tz`.
- API (minimum):
  - `fn now_rfc3339() -> String` → e.g. `2026-10-18T19:05:00+02:00` (seconds precision, explicit offset, never `Z`).
  - `fn from_unix(secs: i64) -> String` (same format).
  - `fn label(secs: i64) -> String` → `2026-10-18 19:05 CEST (+02:00)` (human form from main.md §10).
- Tests (all must pass, offline):
  - 2026-10-18T16:56:00Z → `2026-10-18T18:56:00+02:00`.
  - DST switch: 2026-10-25T00:59:59Z → `2026-10-25T02:59:59+02:00`; 2026-10-25T01:00:00Z → `2026-10-25T02:00:00+01:00`.
  - Winter: 2026-12-01T12:00:00Z → `2026-12-01T13:00:00+01:00`; label says `CET (+01:00)`.
  - Output never ends in `Z` and never contains `+00:00`.
- Wire `openmind-schema` (or a small `openmind` facade) to re-export it as `openmind_schema::time` (optional, document).
- Done when: C1–C3, C6, C7, C8 pass.

## T3 — Galion One, galion-desk, room as workspace crates   (S2)
- Copy sources (read-only originals!):
  - /opt/galion/galion-light/one/src/main.rs → `crates/galion-one/src/main.rs` (+ copy `index.html` etc. it `include_str!`s, if any).
  - /opt/galion/galion-light/desk-rs/main.rs → `crates/galion-desk/src/main.rs`.
  - /opt/galion/galion-light/room/src/main.rs → `crates/room/src/main.rs` (+ index.html if referenced).
- Behaviour must be IDENTICAL. Only allowed changes:
  - bind address/port overridable by env var (e.g. `ONE_BIND`, `ONE_MOUTH_BIND`, `DESK_BIND`, `ROOM_BIND`); default = today's value.
  - paths it reads stay the same absolute paths (do not change data locations).
  - fixes needed only to pass clippy -D warnings (prefer `#![allow(...)]` with a comment over risky rewrites in a 2.8k-line file; list every allow in PROGRESS.md).
- `one` links libsqlite3 via raw FFI: keep it (add `println!("cargo:rustc-link-lib=sqlite3")` build.rs or `#[link]`), do not add rusqlite in this ticket.
- Release builds stripped; record sizes (old vs new) in PROGRESS.md. Target: each new binary < 2 MB.
- Behaviour diff (C5) on spare ports for GET routes that are read-only (`/health`, `/`, `/desk` GET, room `/`). NEVER call POST routes
  of the new desk/one binaries against live state (they write state files) — read the code and list which routes write.
- Do NOT replace /opt/galion/bin/galion-desk or the running `one`.
- Done when: C1–C5, C7, C8 pass.

## T4 — Port desk-hermes.py and link-watch.py to Rust   (S3)
- Read /opt/galion/bin/desk-hermes.py (229 lines, reads Hermes state.db with sqlite3) and
  /opt/galion/galion-light/scripts/link-watch.py (94 lines, writes galion-light/state/link.json). Do NOT run them with python
  (you may run the existing ones only if they are the live path — prefer reading their code and the existing output file).
- New crates `crates/desk-hermes` (bin) and `crates/link-watch` (bin).
  - desk-hermes: same CLI args, same stdout JSON (same keys, same order where practical). Open state.db **read-only**
    (`rusqlite` with `OpenFlags::SQLITE_OPEN_READ_ONLY`, feature `bundled` NOT required: system libsqlite3 is fine) — never write to it.
  - link-watch: same probes and same JSON keys, but output path configurable (`LINK_WATCH_OUT`, default the real path) —
    for tests write to `/opt/galion/openmind/target/test-out/link.json`, NEVER to the live state/link.json. Timestamps via `zagreb-time`
    (a NEW field is fine; keep old fields for compatibility, document any field that was UTC/epoch).
- Golden test: run old vs new on the same input; compare with `jq -S .` (diff must be empty except documented timestamp fields).
  Hermes state.db is busy: copy it first (`sqlite3 /opt/galion/hermes/state.db ".backup /opt/galion/openmind/target/test-out/state.db"`
  if sqlite3 CLI exists, else `cp` the db + -wal) and test against the copy.
- Done when: C1–C3, C5 (golden diff), C6, C7, C8 pass.

## T5 — Galion One systemd unit (STAGED, NOT enabled)   (after T3)
- `deploy/galion-one.service` in the repo: `ExecStart=/opt/galion/openmind/dist/bin/galion-one`, `WorkingDirectory=/opt/galion/galion-light`
  (today's cwd), `Restart=always`, `RestartSec=3`, `Nice=5`, `MemoryMax=256M`, hardening that does not break it
  (`NoNewPrivileges=yes`, `PrivateTmp=yes`). Append-log to /var/log/galion-one.log like today.
- `deploy/switch-galion-one.sh` (bash): preflight (unit file syntax via `systemd-analyze verify`, new binary exists, `/health` of the
  new binary on 18091 returns 200), then: stop the old nohup pid (by pid file/pgrep with exact cwd match), install+start the unit, wait for
  127.0.0.1:8091/health 200 within 20 s; on failure → automatic rollback. `deploy/rollback-galion-one.sh`: stop unit, start the old
  binary exactly as today (`cd /opt/galion/galion-light && nohup ./one/one >>/var/log/galion-one.log 2>&1 &`), check health.
  Both scripts default to DRY-RUN; real run needs `--apply`. Run them ONLY in dry-run. `bash -n` and `shellcheck` (if present) must pass.
- Add entries to TEACHER/STAGED.md ("awaiting Slavik's approval").

## T6 — Console production build (STAGED, NOT switched)   (after T3)
- In a COPY: `rsync -a --exclude node_modules /opt/galion/console-ide/ /opt/galion/openmind/stage/console-src/` then
  `cp -al` or symlink node_modules read-only (do not run `npm install` in the live tree). Build with
  `nice -n 19 /opt/galion/tooling/node/bin/node scripts/with-app-env.mjs node_modules/vite/bin/vite.js build` in the COPY.
  **Do NOT run `npm run build`** — it also runs `db:migrate` against the live database.
- Output to `/opt/galion/openmind/stage/console-dist/` (or the framework default inside the copy, then copied there).
- `deploy/console-prod.service` (staged) that serves the built output on 127.0.0.1:8080 (the way TanStack Start's built server runs:
  `node .output/server/index.mjs` or `vite preview`; document which). Test it on spare port 18080 only, curl `/` and one route, then kill it.
- `deploy/switch-console.sh` + `deploy/rollback-console.sh` (dry-run default, `--apply` required; rollback restores today's unit file
  from a backup copy taken by the switch script). Run only in dry-run.
- Record RAM of the test prod server vs today's dev server (expected: much lower) in PROGRESS.md. Add to STAGED.md.
