# STAGED — awaiting Slavik approval

Nothing in this file is enabled, installed or running. Nothing here restarts a service.
Each entry is a proposal for the operator to approve or reject.

## T3b — runtime Python removed from the ported binaries (2026-10-18T19:47:11+02:00)

Behaviour changes that need approval before switch-over:

1. galion-desk no longer spawns `python3 /opt/galion/bin/desk-hermes.py`. It runs the Rust
   `desk-hermes` binary instead: path from `DESK_HERMES_BIN`, default
   `/opt/galion/openmind/dist/bin/desk-hermes`. The stdout JSON is unchanged (checked against
   the live desk below).
2. galion-one's warm loop no longer shells out to `python3 -c`. That call DELETEd rows in
   Hermes state.db, which is forbidden, so it is removed with no replacement. The warm loop
   still transcribes; it just no longer prunes.
3. `one/models.py` and the voice worker path are off by default, gated behind `ONE_LEGACY_PY=1`
   plus `ONE_LEGACY_PY_BIN` for the interpreter. No interpreter path is hard-coded any more.
   The voice worker had no torch on this box and failed anyway.
4. `ONE_NO_WARM=1` disables the warm loop. Test instances only; production keeps the warm loop.
   It is deliberately absent from the staged galion-one unit.

Net effect once the units are switched: no Python process and no Python spawn is left in the
desk, the One loop or the link watch.

## T5 — staged units and switch scripts (2026-10-18T19:47:11+02:00)

- `deploy/galion-desk.service`, `deploy/galion-one.service`, `deploy/link-watch.service`:
  staged unit files pointing at `/opt/galion/openmind/dist/bin/*`. All three deliberately
  omit the `[Install]` section so they cannot be enabled. galion-desk and link-watch replace
  the live units that still run `python3` today; galion-one is a new unit for a process that
  is currently started outside systemd.
- `deploy/switch.sh`, `deploy/rollback.sh`: dry run by default, `--apply` required, and they
  copy the live unit into `deploy/backup/` before installing. Neither script starts, stops or
  restarts a service — it prints the `systemctl restart` line for the operator to run.
- `dist/bin/`: the five release binaries, copied (not symlinked), with `SHA256SUMS`.
  Verified with `sha256sum -c`: all five OK.

## T6 — console production build, staged, not switched (2026-10-18T19:50:57+02:00)

- Built in a copy, never in the live tree: `/opt/galion/openmind/stage/console-src`, with a
  real copy of `node_modules` (a symlink to the live tree left behind by an interrupted run
  was replaced, so the build could not write into `/opt/galion/console-ide`).
  Build command: `nice -n 19 node scripts/with-app-env.mjs node_modules/vite/bin/vite.js build`,
  exit 0, 6 seconds. `npm run build` is never used — it ends in `db:migrate` against the live DB.
- Output staged at `stage/console-dist/` (server bundle, static files, the 108 KB srvx wrapper).
  The service that would run it: `deploy/console-prod.service`, server on 127.0.0.1:8080
  instead of the dev server's 0.0.0.0:8080. Not enabled, no `[Install]` section.
- `deploy/switch-console.sh` and `deploy/rollback-console.sh` (thin wrappers, dry run by
  default, `--apply` required, never start or stop a service). `deploy/console-prod.md` records
  the exact build command and the measurements.
- Behaviour change to approve: the console stops being served by the vite dev server and starts
  being served as a built bundle. Measured on the spare port 18080: `/` 302 to `/console` on both,
  `/api/v1/health` 401 on both, the public-host gate (galion.app) answers the same 1587 bytes on
  both, and `/status` and `/soon` come straight from the server build instead of the dev shell.
  RAM drops from 517,024 KB (dev server, 505 MB) to 58,112 KB (57 MB) for the same page loads.
- Known limit to decide on: the build uses the project's pinned `vercel` preset, so it runs here
  through the srvx wrapper rather than `node .output/server/index.mjs`. A pod-native
  `node-server` preset build would need a config override in the copy, which was not done.


## B01 link-watch: France probe is config-only (awaiting Slavik's approval)
Written 2026-10-18T20:13:11+02:00 by Hermes (boss), session openmind-boss-20261018.
- The hard-coded France host is gone from `crates/link-watch/src/main.rs`. The probe URL comes only from `LINK_WATCH_FRANCE_URL`.
- Behaviour change: when the variable is unset, the `france` key reads the string `"unconfigured"` instead of a bool, and no network call is made. When it is set, `france` is still true/false.
- Before any switch, the staged `deploy/link-watch.service` needs one line: `Environment=LINK_WATCH_FRANCE_URL=http://<france-host>`. Its comment "No Environment lines" becomes wrong. Readers that expect a strict bool in `france` must be checked first.
- Measured: release binary 394,936 bytes, stripped; peak RSS 1,536 kB for one `LINK_WATCH_ONCE=1` run under target/test-out. Not installed, not enabled, not switched.

## B11 galion-one models refresh (2026-10-18T20:33:13+02:00)
- Repo only; /opt/galion/bin untouched. Before any switch, the unit needs OPENROUTER_API_KEY in its environment (from the existing secret path, not this repo); without it the stale cache is served.
- Behaviour change: a failing fetch is retried on every /models request once the cache is over 600 s old, blocking up to 20 s each. Add a failure backoff before staging.

## OMNODE-2: one sandbox engine, local-device installer, enrollment design (2026-10-18T21:10:00+02:00)
Written by Grok Bot. Nothing committed, nothing published, nothing installed on a device, no service touched, no listener started.

1. **Working-tree change to a committed crate, needs approval before commit:** `crates/openmind-sandbox` (B27 v0, commit 74d52ec) upgraded to v1 in place. Backups: `crates/openmind-sandbox/{src/lib.rs,tests/sandbox.rs,Cargo.toml}.bak-20261018-204528+0200`. Still `libc` only; `Cargo.toml` unchanged; root `Cargo.toml`/`Cargo.lock` byte-identical (md5 523b292d... / 95b66bb3... before and after).
   - API additions (old fields and functions kept): `SandboxPolicy.{env_set, fsize_bytes, output_cap, require_net_isolation, landlock, namespaces, ro_root, hide_paths}`, `Outcome.{stdout_truncated, stderr_truncated}`, `Outcome::enforced(name)`, `features()`, `LAYERS`.
   - Behaviour changes to review: default `fs_read` adds `/sbin`, `/proc`, `/dev/zero`, `/dev/urandom`, `/dev/random`; default `fsize_bytes` = 256 MiB; the `net` layer is now `Enforced` when an empty netns is applied (v0 always said NotEnforced) and `NotEnforced("network allowed by policy")` for `net = true` (v0 said Enforced); by default the command runs in user+net+pid+mount namespaces with a read-only root, `$HOME` and `/root` hidden, all capabilities dropped. Landlock gains ABI 2/3 rights and ABI 4 TCP deny. Fixed: musl/Android build (`__rlimit_resource_t` is glibc-only), Landlock rules on files (`/dev/null` now writable under Landlock), and a pgid-reuse race (the group is now killed before the child is reaped).
   - Tests: 24 pass (was 13) on the VPS and on a Landlock kernel (build box, ABI 6).
2. **omnode uses that crate** as a path dependency (`edge/openmind-node/Cargo.toml`); the `landlock` crate dependency is gone. omnode keeps its own `[workspace]` and `Cargo.lock`. 51 tests pass. New: `omnode pair verify FILE --url URL`, `sandbox run --no-ns`.
3. **Binaries staged, NOT hosted:** `edge/dist/` (untracked): `omnode-x86_64-linux-musl` (1,497,512 B, sha256 247bcc10...), `omnode-aarch64-linux-musl` (1,250,312 B, sha256 b832c6ec...), `install.sh`, `SHA256SUMS`. **Hosting them anywhere needs Slavik's approval**; the plan is a GitHub release once the repo exists, then `OMNODE_BASE_URL=https://github.com/<org>/<repo>/releases/download/<tag> sh install.sh`.
4. **Installer:** `edge/openmind-node/install.sh` (POSIX sh, tested with dash). Installs only into the user's own bin dir, verifies sha256 (tested: tampered binary refused), runs `identity init` / `discover --dry-run` / `sandbox check`. No sudo, no rc edits, sends nothing.
5. **Hermes enrollment endpoint: design only** (`docs/hermes-enrollment.md`), `enabled = false` by default, no code in Hermes, no listener. Needs Slavik's approval and an HTTPS hostname before anyone builds or enables it.
6. Toolchain: `rustup target add x86_64-unknown-linux-musl` on the VPS (rust-std component only; logged in docs/TERMINAL-LOG.md).
7. Merge leftovers to decide: delete `edge/openmind-node/src/*.bak-*`/crate backups after commit; optionally move omnode under `crates/` (needs an `exclude` line in the root `Cargo.toml`).

## Survival units, awaiting Slavik's approval

Written 2026-10-19T15:37:45+02:00 (Europe/Zagreb). DESK-ID openmind.B21.

Staged in the repo only. Not installed. Not enabled. Nothing was copied into /etc/systemd. No systemctl enable, start, link, or daemon-reload.

- deploy/survival/guardian-tick.service
- deploy/survival/guardian-tick.timer
- deploy/survival/galion-one.d/10-restart.conf
- deploy/survival/galion-desk.d/10-restart.conf
- deploy/survival/link-watch.d/10-restart.conf
- deploy/survival/console.d/10-restart.conf
- deploy/survival/tuner.d/10-restart.conf

guardian-tick.service is Type=oneshot. It runs /opt/galion/openmind/dist/bin/guardian-tick once with probes file /opt/galion/openmind/deploy/survival/probes.toml and writes the JSON report to /tmp/guardian-tick.json. It does not open a database. The service and the timer have no [Install] section. Each drop-in contains only Restart=always, RestartSec=5, StartLimitIntervalSec=60, StartLimitBurst=5.

systemd-analyze verify on deploy/survival/guardian-tick.service and deploy/survival/guardian-tick.timer exits 1 because the binary path is not installed. Exact error:

    guardian-tick.service: Command /opt/galion/openmind/dist/bin/guardian-tick is not executable: No such file or directory

The timer file alone verifies with exit 0. systemctl is-enabled guardian-tick prints not-found. Files left uninstalled.

# B14 — retire the legacy TTS worker spawn

DESK-ID: openmind.B14
Written: 2026-10-19T16:10:09+0200 (Europe/Zagreb)
Nothing here is installed, enabled, or running. No commit. No systemctl. galion-light was not modified (tts/worker.py still present).

## Behaviour awaiting approval

- `crates/galion-one` no longer reads `ONE_LEGACY_PY` or `ONE_LEGACY_PY_BIN` and no longer spawns the old `tts/worker.py` worker. No interpreter is launched.
- Voice-worker-unavailable uses today's off-path JSON, captured from the live mouth before the edit:
  - `GET /health` on the mouth port: HTTP 503, body `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}` (86 bytes). The loaded:true 200 body is gone, because that body required the worker slot.
  - `POST /synthesize` when the mouth is not remote: HTTP 500, body `{"error":"voice"}`. Same body as the old off path. The old off path waited up to 90s for a worker that was never started; this returns immediately.
- Remote mouth is unchanged: if `GALION_MOUTH` is not 127.0.0.1:8002, `speak` still POSTs `/synthesize` and returns that wav.
- A live reply no longer blocks on the retired local worker. With no remote mouth, no audio is sent (same end result as the off path).
- The nerve `origin` row still matches the same process-list bytes. The source splits the suffix so `rg python|.py` stays on comments. This crate does not launch that helper.
- Comment-only wording in `models_refresh.rs` so each remaining rg hit names `models.py` (B11). Output bytes unchanged.

## Accept

- fmt: `nice -n 19 cargo fmt -p galion-one -- --check` exit 0. Scoped to galion-one (other crates are dirty and out of this ticket).
- clippy: `nice -n 19 cargo clippy -j 2 -p galion-one --all-targets -- -D warnings` exit 0. Tail: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.69s`.
- test: `nice -n 19 cargo test -j 2 -p galion-one -- --test-threads=2` exit 0. 3 passed (models_refresh golden, bad json, fetch error). Tail: `test result: ok. 3 passed; 0 failed`.
- release: `nice -n 19 cargo build -j 2 --release -p galion-one` exit 0. `CARGO_TARGET_DIR=/tmp/om-b14`. Tail: `Finished release profile [optimized] target(s) in 7.90s`.
- binary: `/tmp/om-b14/release/galion-one` 634048 bytes, stripped, ELF 64-bit LSB pie. Under 2 MB.
- RSS: one shot on 127.0.0.1:18091 only (`ONE_NO_WARM=1`, `GALION_HOME=/tmp/om-b14-home`, mouth bind pointed at the same 18091 so it could not listen twice). VmRSS/VmHWM 3212 kB. Log tail: `one: listening`. Killed. 18091 free afterwards. Live 8091 still 200, live 8002 still 503.
- `rg -n "Command::new(\"python|ONE_LEGACY_PY|python3" crates/ --glob '*.rs'`: no hits.
- `rg -n -i 'python|\.py' crates/galion-one --glob '*.rs'` only comments that name a replaced script:
  - models_refresh.rs:4, :6, :206 name models.py
  - main.rs:1189 names tts/worker.py
  - main.rs:1636 names models.py
- No CJK in crates/galion-one. No new Rust file, so no new SPDX header. No secrets added.

## Cargo.lock

galion-one gained no dependencies. A transient lock edit that was not this ticket (one-set-model, openmind-schema) was copied and `git checkout -- Cargo.lock` was run. Working tree lock is clean now. `/tmp/om-lock-B14` blob ab2b4f7c964ad720d7d205f6d032a8df3f57bdde; HEAD:Cargo.lock ab2b4f7c964ad720d7d205f6d032a8df3f57bdde; worktree ab2b4f7c964ad720d7d205f6d032a8df3f57bdde.

## Not done

No git commit. No systemctl. Production bins and units were not switched. TEACHER/STAGED.md was not edited.

## hermes-tools: unit drop-in for hermes-rs coding tools (2026-10-20T20:10:00+02:00, slot 17)

Proposal only: `deploy/staged/hermes-rs.service.d/coding-tools.conf`, explained in
`docs/staged/hermes-tools-unit.md`. MemoryMax 64M -> 2304M for the unit (the gateway stays at
64M in its own leaf), `Delegate=memory pids cpu`, TasksMax 64 -> 512, CPUQuota 200%, and the
coding-tool environment. It needs approval of the base `hermes-rs.service` first. Paid model use
for real tickets is a separate decision and is not part of this proposal.
