---
tags: [galion, openmind, rust-port, galion-one, galion-desk, tuner]
project: galion
status: staged, awaiting Slavik's approval
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Workstream one-desk-tuner: Galion One, galion-desk, tuner

Written 2026-10-18T21:20+02:00 (Europe/Zagreb) by Grok Bot on branch `ws/one-desk-tuner`. Plan phase 0 (B51, B52, B53, the B21 unit for One)
plus the Python left in Galion One (B11 follow-up, B12, B13, B14, B15, part of B05/B06). All builds were done on the box with `nice -n 19` and `-j 2`
(`-j 1` for cross builds). The VPS was read only: sources copied out, live routes fetched with GET. Nothing there was changed, restarted or installed.

## 1. Status

| Component | Status | Notes |
|---|---|---|
| Galion One: repo crate vs live source | **done** | The live `galion-light/one/src/main.rs` (2,806 lines, 13 Oct, same mtime as the live binary) after rustfmt is 3,654 lines. The repo crate was already a superset. The only live item the repo lacked is `trim_warm`, an inline `python3 -c` that deleted the `one-warm` session from Hermes `state.db`. It is not ported on purpose (Galion One never writes state.db), see section 5. |
| Galion One: no Python | **done** | No Python spawn and no `ONE_LEGACY_PY*` gate left. `rg -i 'python\|\.py'` finds only comments naming replaced scripts, plus the `origin.py` ps-text detector in the nerve unit list (it reads `ps`, launches nothing; B56 belongs to the origin decision). |
| Galion One: models refresh backoff | **done** | The fix asked for in STAGED.md B11. |
| Galion One: speech seam | **done (stub)** | `speech::Mouth` trait with an `Unavailable` stub. galion-voice-core plugs in here. |
| Galion One: persona seam | **done** | `persona.rs` holds the Samantha and Jarvis text and the admin-line text, byte for byte. openmind-persona plugs in here. |
| galion-desk | **done** | Config-driven (B05), release build 403 KB instead of the 4.7 MB debug build. Same responses. |
| tuner + quick-tunnel publisher | **done** | New `crates/tuner` (B53): `tuner` gives byte-identical responses, and `tuner-publish` replaces the transient `tuner-quick.service`. |
| Staged units + switch steps | **done, not installed** | `deploy/staged/{galion-one,tuner,tuner-publish}.service`. galion-desk uses the existing staged `deploy/galion-desk.service`. |
| Live parity, galion-desk `/desk` | **partial** | Not fetched from the live desk: each live GET /desk runs `python3 desk-hermes.py` on the VPS, and this stream does not trigger Python. The code diff against live is only the env lookups (section 4). |

## 2. What changed (commits on ws/one-desk-tuner)

- `635fe4a` galion-desk: config-driven paths and Ollama address (B05), with the live values as defaults, plus an HTTP test on a spare port.
- `9519693` galion-one:
  - **B14:** the `tts/worker.py` spawn, `ONE_LEGACY_PY`/`ONE_LEGACY_PY_BIN`, `Worker`, `boot_worker`, `start_worker` and `read_full` are removed. The local mouth is now `speech::local()`, the `Unavailable` stub. `:8002 /health` still answers 503 `{"ok":false,"status":"loading",...}`, and `/synthesize` still answers 500 `{"error":"voice"}`. Both now answer at once; the live binary waited 90 s for a worker that never comes.
  - **B12 and memory counts:** the hand-written libsqlite3 FFI is gone, in both `load_defence` (guardian.db, the old `defence.py`) and `memory_file_counts` (memory-gateway store). It is replaced by `sqlite_ro.rs`, a pure-Rust read-only SQLite reader: rowid and WITHOUT ROWID b-trees, overflow pages, committed WAL frames with salt and checksum checks. `build.rs` is deleted, so no system libsqlite3 is needed. The old FFI also called `sqlite3_open` read-write, which could create an empty guardian.db; the new reader only uses `File::open`.
  - **B11 follow-up:** a failure backoff for the models refresh (60, 120, 240 ... 3600 s, reset on success), single flight (only one curl at a time). The key comes from `OPENROUTER_API_KEY` or from the `OPENROUTER_API_KEY=` line of `OPENROUTER_KEY_FILE`, which is how models.py read it.
  - **B15:** `galion-one set-model <plan|action> <provider/model>` replaces set-model.py. It writes the same indent=2 JSON with a trailing newline, now atomically. `galion-one trace <src.pgm|ppm> <dest.svg> [--no-caption]` replaces trace.py, using the same algorithm and the same SVG.
  - The `sha256sum` child that ran on every token check is replaced by in-process SHA-256 (FIPS test vectors).
  - `persona.rs` added. **B06 (partial):** `ONE_DEVICE_STORE`, `ONE_HERMES_DIR`, `ONE_HERMES_ENV`, with the live paths as defaults.
- `449a009` UNIVERSAL-CPU.md committed.
- `4c65d96` crates/tuner (B53).
- `61a15f8` `ONE_WARM_MIND=0` switch, plus a comment on the trim_warm that is not ported.
- `9a6c6e1` staged units.

### Python removed from Galion One (442 lines in galion-light/one)

| Script | Lines | Now |
|---|---|---|
| talk.py | 230 | Already replaced by the Rust handlers `/me`, `/pair`, `/hear`, `/reply` (same store format, tokenHash/tokenHashes, 5-fail 15-min lock, same channel files). No caller. Its last spawned dependency, `sha256sum`, is now in-process. |
| trace.py | 131 | `galion-one trace` (input is PGM/PPM; the Python used Pillow for JPEG/PNG, so convert first). No caller. |
| warm.py | 35 | Already `warm_pass`/`keep_warm`. No caller. |
| models.py | 24 | `models_refresh.rs` (B11) plus the backoff and the key file. |
| set-model.py | 12 | `galion-one set-model`. |
| defence.py | 10 | `load_defence` through `sqlite_ro` (golden checked against a real guardian.db copy). |
| inline `trim_warm` (python3 -c, 11 Rust lines) | 5 py | Dropped on purpose (state.db write). |

The .py files themselves are still on the VPS. Deleting them is part of the switch, after approval (section 7, step 9).

## 3. Tests, lint, sizes, RSS

Tests: galion-one has 25 unit tests, galion-desk 3 unit + 1 HTTP test, tuner 4 unit + 2 end-to-end tests: **35 in total, all pass**.
`cargo fmt --check` passes, and `cargo clippy -p <crate> --all-targets -- -D warnings` is clean for all three crates.
Goldens: the guardian fixture and the WAL fixture were generated with real SQLite (rusqlite bundled, in a scratch program outside the repo, never committed).
The sqlite_ro output also equals real SQLite on a copy of the live guardian.db (`ONE_TEST_GUARDIAN_DB`/`ONE_TEST_GUARDIAN_EXPECTED`, optional test).

Release binaries (opt-level z, lto, strip; `readelf` shows no .symtab):

| Binary | x86_64-gnu | x86_64-musl | aarch64-musl | armv7-musleabihf | Live today |
|---|---|---|---|---|---|
| galion-one | 690,848 | 821,832 | 731,784 | 708,684 | 1,060,288 (not stripped, needs libsqlite3) |
| galion-desk | 403,072 | 529,832 | 476,216 | 444,508 | 4,735,088 (debug) |
| tuner | 325,184 | 444,104 | 399,240 | 369,380 | ~509 KB static-pie |
| tuner-publish | 369,152 | 462,104 | 407,768 | 381,316 | (cloudflared transient unit) |

Peak RSS (VmHWM) on x86_64 after serving the read-only routes: galion-one is 3.6 MB on gnu and 1.0 MB on musl static, against 10.3 MB live.
galion-desk is 2.4 MB on gnu and 0.5 MB on musl, against 1.7 MB live. tuner is 2.0 MB on gnu and 0.4 MB on musl, against 264 KB live (static-pie).

## 4. Behaviour parity checklist (vs live)

- [x] Galion One, 14 read-only routes on 8091 (`/ /nerve /admin /mark.svg /space /api/v1/catalog /health /me /models /model /api/v1/analytics /session /files /nope`) and `8002/health`: **byte-identical** to live, Date header removed, unauthenticated, with a paired fixture store. The static files used are the live ones (sha256 checked).
- [x] Pairing: a wrong code gives `{"error":"no"}`; the right code sets the `galion_admin` cookie, and `/me` then answers `{"you":true}` (in-process SHA-256).
- [x] Samantha/Jarvis persona text unchanged (test asserts the exact text); the Hermes channel files and the session headers are unchanged.
- [x] Keep-warm: same schedule (20 s start, `warm_due` 240 s, `state/warm.ts`). `ONE_NO_WARM=1` and `ONE_WARM_MIND=0` are opt-outs, default off.
- [x] Models refresh: same cache format (golden). A refresh that fails no longer blocks every `/models` call for up to 20 s.
- [~] Mouth on 8002: same status codes and bodies, answered at once instead of after 90 s. It is still not a working voice until galion-voice-core lands.
- [~] Not done any more: the one-warm pruning in Hermes state.db (section 5).
- [x] galion-desk: the source diff against live `desk-rs/main.rs` is only env lookups (`DESK_BIND`, `DESK_STATE_DIR`, `DESK_ONE_DIR`, `DESK_HERMES_DIR`, `DESK_OLLAMA`, `DESK_HERMES_BIN`). The defaults equal the live constants (unit test). The mind pack now comes from the Rust `desk-hermes` instead of `python3 desk-hermes.py`; its parity is the desk-hermes crate's job.
- [x] tuner: `GET`, `HEAD` and `POST` on `/ /index.html /app.js /pitch.js /style.css /health /nope /?a=1` are **byte-identical** to live 8650 (25,357 bytes both).
- [x] tuner-publish: runs the same cloudflared command line (`tunnel --no-autoupdate --url http://127.0.0.1:8650`), checked with `--dry-run` and against a fake cloudflared.

## 5. Needs Slavik

1. **Approve the switch** (section 7): the One unit, then the desk and tuner binaries.
2. **The one-warm session in Hermes state.db:** live One deleted it with python3 after each warm pass. The Rust One never writes state.db, so with the default settings Hermes keeps one more short exchange about every 4 to 5 minutes. Options: (a) accept it, (b) a Hermes-side cleanup job, (c) `ONE_WARM_MIND=0` (no Hermes warm ping).
3. **FIXED on ws/ui 15cdb53 (fail closed, now live):** formerly open when unpaired: if the device store file is missing or has no `codeHash`, One treats every caller as paired. That includes `/files`, `/models` and `/api/v1/analytics`. The staged unit names the live store path explicitly. Consider fail-closed before One is reachable through any tunnel.
4. **O4:** keep publishing the tuner on a random trycloudflare URL (`tuner-publish.service`), or stop `tuner-quick`.
5. **OpenRouter key:** the unit points `OPENROUTER_KEY_FILE` at `/workspace/hermes/.env`, the same file models.py read. Confirm, or give One its own key file.
6. **Delete** `galion-light/one/{talk,trace,warm,models,set-model,defence}.py` and `__pycache__` once the switch has run for a day (not done).

## 6. Integration points

- **Persona:** `crates/galion-one/src/persona.rs`. `for_voice()` returns `BuiltIn` today. When `crates/openmind-persona` (ws/hermes) lands, return an adapter implementing `Persona` (`id`, `voice_id`, `voice_setting`) plus `admin_line()`. main.rs needs no other change.
- **Voice:** `crates/galion-one/src/speech.rs`. `local()` returns the `Unavailable` stub. `galion-voice-core` (ws/voice) implements `Mouth` (`ready`, `synth(voice, text, sink)`, 16-bit LE mono PCM at 24 kHz). It can also run a CPU TTS child behind the kept framed protocol (`frame_request`, `read_ready`, `read_frames`). Port 8002 then becomes a thin adapter. A remote mouth (`GALION_MOUTH`) still goes over HTTP.
- **SQLite reader:** `sqlite_ro.rs` is self-contained, std only. Other streams that only need read-only counts or scans can copy it instead of pulling in rusqlite bundled, which needs a C cross compiler.

## 7. Switch steps (for Slavik's approval; nothing below has been run)

Binaries are built on the box, never on the VPS. Use x86_64-musl (static, smallest RSS) or x86_64-gnu.

1. On the box: `CARGO_TARGET_DIR=/workspace/oom-target/one-desk-tuner nice -n 19 cargo build -j 2 --release --target x86_64-unknown-linux-musl -p galion-one -p galion-desk -p tuner` (linker `rust-lld`), then `sha256sum` the four bins.
2. Copy them to the VPS into a new dir: `scp <bins> root@VPS:/opt/galion/openmind/dist/bin-ws/`. On the VPS, check the same sha256 and run `./galion-one version`.
3. Dry check without touching live ports: `GALION_HOME=/workspace/galion-light ONE_BIND=127.0.0.1:18091 ONE_MOUTH_BIND=127.0.0.1:18002 ONE_NO_WARM=1 /opt/galion/openmind/dist/bin-ws/galion-one &`, then the C5 diff of the read-only routes against 8091 (command in TEACHER/CHECKS.md), then kill it.
4. Install the One unit: `install -m 644 deploy/staged/galion-one.service /etc/systemd/system/galion-one.service && systemctl daemon-reload`.
5. Cut over (about 5 s without One): `kill 1013026` (the nohup ./one/one), `systemctl start galion-one`, `curl -s 127.0.0.1:8091/health`, `curl -s -i 127.0.0.1:8002/health` (expect 503 loading), `tail /var/log/galion-one.log`.
6. Boot hook (the unit has no [Install], on purpose): `mkdir -p /etc/systemd/system/multi-user.target.wants && ln -s /etc/systemd/system/galion-one.service /etc/systemd/system/multi-user.target.wants/`.
7. galion-desk: copy `dist/bin-ws/galion-desk` over the path in the staged `deploy/galion-desk.service` (or use `deploy/switch.sh galion-desk`, a dry run first), then `systemctl restart galion-desk` and `curl 127.0.0.1:8646/health`.
8. tuner: `install -m 644 deploy/staged/tuner.service /etc/systemd/system/tuner.service`, `daemon-reload`, `systemctl restart tuner`. The live unit's `[Install]` link stays in place. If O4 says keep publishing: stop the transient `tuner-quick`, install `tuner-publish.service`, start it, and read `/run/tuner/quick-url.json`.
9. After a day without problems, delete the six .py helpers (item 5.6).

Rollback: `systemctl stop galion-one`, then from `/opt/galion/galion-light` run `nohup ./one/one >>/var/log/galion-one.log 2>&1 &`. For desk and tuner, restore the old ExecStart or binary and `systemctl restart`. The old binaries are not touched by any step.

## Universal CPU

How the plan changed: galion-one linked the system `libsqlite3.so.0` through hand-written FFI. That library does not exist on musl, aarch64, armv7 or Android without a C cross toolchain, so the FFI was replaced by the pure-Rust read-only reader instead of rusqlite bundled (which needs C).
The other changes: in-process SHA-256 instead of a `sha256sum` child; JSON, PNM decoding and HTTP are std only. There are no new external crates: tuner uses only the workspace `zagreb-time`. Nothing is arch-specific (no asm, intrinsics or target flags).
External programs galion-one still calls when they are present: `curl` (models refresh, edge probes), `ps`, `pdftotext`, `nvidia-smi`. Each is optional, and the code degrades when one is missing.

| Target | Tier | galion-one | galion-desk | tuner / tuner-publish |
|---|---|---|---|---|
| x86_64-unknown-linux-musl | 1 | build ok, runs (RSS 1.0 MB) | build ok, runs | build ok, runs |
| aarch64-unknown-linux-musl | 1 | build ok | build ok | build ok |
| armv7-unknown-linux-musleabihf | 2 | build ok | build ok | build ok |
| aarch64-linux-android | 2 | `cargo check` ok | `cargo check` ok | `cargo check` ok |

Linker for the musl targets: `CARGO_TARGET_<TRIPLE>_LINKER=rust-lld`. Sizes are in section 3. The aarch64, armv7 and Android builds were not run (no emulator on the box).
