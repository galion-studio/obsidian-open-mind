<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# opt-bin-py-port (wave-4 slot 270): real tests

Branch `swarm/opt-bin-py-port`, base main-staging 7c95348b. All times Europe/Zagreb (CET +01:00).
Box ports 21400-21409 only. VPS access was read-only (ls, cat of the three helper sources, systemctl show,
ss, GETs to nerve/cortex, one `ssh -L` tunnel for GETs to the live console). No secret file was read
(only `ls` showed `replicate.key` and `pw-cookie` exist; their contents were never opened).

## Inventory: /opt/galion/bin on the VPS (2026-10-27 18:23 +01:00)

| File | Size / lines | State before this branch | Result |
|---|---|---|---|
| desk-hermes.py | 229 lines | already ported: `crates/desk-hermes` (rusqlite bundled), a Rust build is on the VPS at `/opt/galion/openmind/dist/bin/desk-hermes` (1,427,792 B, 2026-10-18 19:43 +02:00) | nothing new to port; live galion-desk (`/opt/galion/bin/galion-desk`, no Environment) still spawns `python3 /opt/galion/bin/desk-hermes.py` (string seen in the binary) -> that cutover is docs/cutover/desk.md, not this slot |
| pw.py | 58 lines, Playwright + venv | not ported | **new `crates/pw-check`** |
| replicate-picture.py | 92 lines, urllib | not ported | **new `crates/replicate-picture`** |
| galion-desk | 4,735,088 B ELF | Rust already | - |
| `__pycache__/` | dir | Python bytecode | archive with the .py files after install (plan) |

No unit, cron or Hermes config references pw.py or replicate-picture.py (grep of /etc/systemd/system,
/etc/cron*, /opt/galion/hermes/config*: 0 hits). No process was running any of the three at 18:32 +01:00.

## replicate-picture

The real Replicate API is paid: never called. `real_replicate_picture.rs` (GALION_REAL=1) starts the release
binary against an in-test mock of the Replicate HTTP API, real system curl, and while serving every request
scans all `/proc/*/cmdline` for the token.

| Scenario | Port | Result |
|---|---|---|
| S1 flux-dev 422 -> flux-schnell `processing` -> poll x2 -> `succeeded` list output -> download | 21400 | exit 0, stdout `black-forest-labs/flux-schnell\n<dest>\n`, file bytes equal, request order POST dev, POST schnell, GET p1, GET p1, GET file; Bearer + `Prefer: wait=60` on the 4 API calls, NO Authorization on the download, token on no argv, prompt with quotes + newline round-trips, aspect 16:9, png, num_outputs 1 |
| S2 direct string output | 21401 | exit 0, flux-dev, 2 requests |
| S3 both 500 | 21402 | exit 1, stderr `500 {"err":"down schnell"}` (last error, Python format), no file |
| S4 prediction `failed` while polling | 21403 | exit 1, `NSFW content detected`, no fallback (as Python's SystemExit) |
| S5 no key file | 21404 | exit 1, `missing replicate key file`, 0 requests |
| S6 nothing listening | 21405 | exit 1, `URLError curl: (7) Failed to connect ...`, no token in stderr |

Runs: 18:24 (first), 18:35:3x x3 (`GALION_REAL=1 cargo-slot test --release -p replicate-picture`) all pass; unit 6/0.

## pw-check

`real_pw_check.rs` (GALION_REAL=1) runs the release binary against a real headless Chromium (box: /usr/bin/google-chrome)
and an in-test console stand-in on 127.0.0.1:21406.

- S1 4 pages (`/console auth /wide /uni`): title, h1 trimmed (`["Console","Second"]`), svgs 3, theme `Use dark`,
  dock `["Desk","Nerve","Defence"]`, text whitespace-collapsed, text includes `LATE-LOADED` from a 700 ms fetch
  (networkidle really waits), errors `["boom-21406"]` (sync throw) and `["late-type"]` (TypeError in setTimeout),
  cookie reaches the server (`authed`), `wide` true for a 2000 px div, non-ASCII title escaped `\u00b7 \u017e`,
  key order = Python dict order, stdout == report.json, 4 PNGs 390x844, cookie value in neither stdout nor stderr,
  no Chromium child left, no profile dir left. 8.3-12.5 s for 4 pages.
- S2 no cookie file -> `anon`.
- S3 nothing on 21407 -> exit 1 `Page.goto: net::ERR_CONNECTION_REFUSED at ...`, no report.json, no profile dir left
  (this caught a real bug: SIGKILL left helpers writing into the profile; fixed with graceful Browser.close, 4835045).
- Runs: 18:28 (first), 18:34-18:35 x3 after the fix: all pass; unit 6/0.

### Live console comparison (read-only)

18:31:32-18:32:04 +01:00: `ssh -L 21408:127.0.0.1:8080` to the VPS, then on the box
`pw-check --base http://127.0.0.1:21408 --out-dir scratch/... --cookie-file /nonexistent/pw-cookie` (default 7 paths,
deliberately without the admin cookie). Exit 0 in ~32 s; every page `title "Galion Console"`, h1 `["This phone"]`,
svgs 0, theme null, dock [], wide false, errors 0, text "ADMIN This phone One device. Enter the pairing code once. ..."
= the pairing gate an unpaired browser should see; the 390x844 screenshot shows the "Pair this phone" screen.
report.json == stdout. The last Python report on the VPS (`/opt/galion/hermes/cache/pw/report.json`, 2026-10-10
19:02 +02:00, 8 rows) has the same keys in the same order (`title,h1,svgs,theme,dock,wide,text,path,errors,shot`)
and the same byte layout (`[\n  {\n    "title": ...`). :8080 is served by `node` (pid 138369) at 18:23 +01:00.

### Live health during the work (read-only GETs)

18:32:49 +01:00: nerve 127.0.0.1:8670 /state score 78 grade C; cortex 127.0.0.1:8674 /health ok, mode live,
incidents_open 2, escalations_open 0 (not caused by this slot: nothing on the VPS was changed or started).

## Gates

- `cargo fmt --check` (both crates), `cargo clippy --all-targets -D warnings` (both), `scripts/no-python.sh` ok,
  `scripts/spdx-scan.sh` missing 0 (742 files); desk-hermes tests 4+9/0 (unchanged crate, still green).
- x86_64-unknown-linux-musl release: pw-check 594,720 B, replicate-picture 492,216 B, statically linked; smoke:
  `--help`, missing key exit 1, pw-check nav failure exit 1 with no profile dir left.
- aarch64-unknown-linux-musl release (rust-lld): pw-check 525,008 B, replicate-picture 432,048 B.
- `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf: ok. armv7-linux-androideabi is not an
  installed target on the box (std missing), not a code issue.
- desk-hermes x86_64 musl does not build on this box (bundled SQLite needs `x86_64-linux-musl-gcc`; pre-existing,
  not touched here).
- Full workspace test not run: the branch only adds two new leaf crates (Cargo.lock: +2 local packages, no new
  registry crates), no existing crate changed.
