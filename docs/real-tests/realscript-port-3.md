<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests: realscript-port-3 (wave 4 slot 114)

Port of the **last third (alphabetical)** of `scripts/real/*.sh` on integration-2
(df42e0f) to Rust `GALION_REAL=1` tests (Rust-only rule, 2026-10-20 20:41). The
scripts are deleted. All times Europe/Zagreb (CEST, +02:00). Box: 8 vCPU, Linux 6.12.
Ports: slot 114 range 127.0.0.1:19840-19849 (real_stack keeps the harness's own
19500-19551 layout). The 27 entries of `scripts/real/` sorted: entries 1-9 are
realscript-port-1, 10-18 are realscript-port-2, 19-27 are this slot.

| Deleted script | Rust replacement (all in `crates/galion-realtest/tests/`) | Port |
|---|---|---|
| `real-harness.sh` | `real_stack.rs` (`real_box_stack` + new `real_live_probe_read_only`); now builds its own release binaries and includes `openmind-nerve` | 19500-19551 |
| `room.sh` | `real_sh_room.rs` | 19840, 19841 (netns) |
| `sandbox.sh` | `real_sh_sandbox.rs` (runs `openmind-sandbox`'s `real_escape` suite unprivileged in a delegated cgroup) | none |
| `survival-heal.sh` | `real_sh_survival_heal.rs` | 19842, 19843 |
| `tuner-capture.sh` | `matrix()` + `live_capture()` in `real_sh_tuner_cutover.rs` (`ssh -W 127.0.0.1:8650`, GET only) | 8650 (VPS, read-only) |
| `tuner-cutover.sh` | `real_sh_tuner_cutover.rs` | 19844 |
| `tuner-requests.txt` (data) | moved to `crates/tuner/tests/fixtures/requests.txt` | |
| `ucpu.sh` | `real_sh_ucpu.rs` + `src/bin/galion-ccwrap.rs` (Rust zig-cc wrapper) | 19845-19849 |
| `voice-mouth.sh` | `real_sh_voice_mouth.rs` | 19840-19842, 19849 (dead mind) |

Shared plumbing: `tests/sh3/mod.rs` (gate, build fallback into
`$CARGO_TARGET_DIR/real-bins`, process guard that kills on drop, raw HTTP, ELF
facts without readelf, leftover-process scan). Same env names as port-1/-2:
`GALION_REAL`, `GALION_REAL_VPS`, `GALION_REAL_BIN_DIR`, `GALION_REAL_JOBS`, `GALION_REAL_KEEP`.

## Run (2026-10-21 03:33:41 - 03:36:04 CEST)

```
export CARGO_TARGET_DIR=/workspace/swarm/target/realscript-port-3
cargo-slot build --release --locked -j 2 -p galion-realtest -p galion-boot -p galion-one -p openmind-memory \
  -p tuner -p galion-desk -p link-watch -p guardian-tick -p persona-plex-relay -p room
cargo-slot build --release --locked -j 2 -p guardian-tick --bins --example dummy_service
cargo-slot build --release --locked -j 2 -p openmind-sandbox --example escape
cargo-slot test  --release --locked -j 2 -p openmind-sandbox --test real_escape --no-run
cargo-slot build --release --locked -j 2 -p galion-voice-cpu --features whisper,kitten --bin galion-voice-serve
# tuner x86_64/aarch64 musl and the 4 UCPU packages for aarch64/armv7 musl (zig via galion-ccwrap)
GALION_REAL=1 GALION_REAL_VPS=1 GALION_REAL_BIN_DIR=$CARGO_TARGET_DIR/release \
  cargo-slot test -j 2 -p galion-realtest -- --nocapture --test-threads 1
```

Full verbatim output: `docs/real-tests/captures/realscript-port-3-20261021.log` (622 lines, no secrets).
Without `GALION_REAL_BIN_DIR` every test builds what it needs itself.

| Test | Result | Highlights |
|---|---|---|
| real_sh_room | 19/19 PASS, 1.4 s | hostile reads all `404 held` (secret, symlink escape, /etc/passwd), bad escape no crash, POST 405; netns room unreachable from host, `ok` inside via nsenter, only `lo` in the ns; no-flag page **0 network syscalls** under strace, `--listen` control shows socket/bind/listen; no leftover processes |
| real_sh_sandbox | PASS, 2.1 s | all 14 attacks CONTAINED as uid 1000 in `/sys/fs/cgroup/om-sandbox-real-<pid>/runner`: setuid-root `id` stays uid 1000 (no_new_privs), fork bomb stopped after 22 forks (pids.max), memory bomb `Signaled(9)` (memory.max + oom.group), setsid orphan reaped by cgroup.kill; cgroup and helper removed |
| real_sh_survival_heal | 16/16 PASS, 18.7 s | real dummy died, dry-run did not act, 3 applied restarts each a NEW pid verified healthy, 4th tick `cap-reached` with no restart; incident store + nerve JSONL `+02:00`, no `Z`; `classify` over the captured live status.json: 12/12 healthy |
| real_sh_tuner_cutover | PASS, 60.5 s | static x86_64 (471,688 B) + aarch64 musl, SHA256SUMS; **19/21 routes byte-identical with live VPS 8650, 2 KNOWN-DIFF** (see below) |
| real_sh_ucpu | 38/38 PASS, 0.7 s | aarch64 + armv7 static galion-one, tuner, openmind-memory (SQLite write+read), hermes-gateway `--mock` all answer under qemu-user; RSS 11-21 MB each |
| real_sh_voice_mouth | 24/24 PASS, 40.1 s | JFK STT 2.46-2.60 s (1 thr) / 1.46-1.51 s (4 thr); TTS 5.83 s audio in 2.61-3.16 s; round trip 5.80 s (4 thr); VmHWM 449 MB; galion-one mouth proxies to the voice service (200, RIFF), without `GALION_MOUTH` an honest 503 |
| real_stack (real_box_stack) | PASS | 12 scenarios PASS incl. one-fail-closed (fixed on integration-2), nerve-ingest SKIP in the logged run; after adding `openmind-nerve` to the build list and `--with-nerve` (03:39 CEST re-run, 11.4 s) all 13 PASS incl. nerve-ingest |
| real_stack (real_live_probe_read_only) | PASS | live VPS read-only: 43 checks, 41 pass, 0 FAIL, 2 known-fail (mouth 8002 `loading`, link-watch.service inactive) |

## Finding: tuner parity on integration-2 (owner decision)

Since 26d98b0 (swarm/common, merged into integration-1/-2) the tuner reads
requests through the strict galion-common HTTP parser. Two malformed request
lines in the matrix now get `400 Bad Request` (120 B) where live 8650 and the
recorded fixtures serve the page (`GET\r\n\r\n`, 2,700 B) or health
(`GET /health\r\n\r\n`, 335 B). All 19 valid requests are still byte-identical.
`crates/tuner/tests/real_tuner.rs` (`real_binary_matches_live_on_every_route`)
**fails** on integration-2 for the same reason; `golden_live.rs` still passes
because it calls `tuner::respond` directly and skips the server.
Options: (A) accept the stricter 400, re-record those 2 fixtures and update
the cutover doc's "21/21" to "19 + 2 intentional"; (B) make the tuner's server
pass these legacy forms through to `respond` to keep full parity. Until then
`real_sh_tuner_cutover` reports them as KNOWN-DIFF (`TUNER_STRICT_PARITY=1`
makes them fail). The x86_64 musl sha256 is now `2e02a770…9040` (not
`47a17df2…` from docs/cutover/tuner.md), as expected after the crate changed.

## Other observations
- openmind-memory `/search/conversations` prints message stamps as UTC `…Z`
  (e.g. `2026-10-21T01:29:31.570Z`); not changed here (pre-existing, project time rule).
- `ssh -W` costs ~3 s per route (21 ssh sessions); fine for a manual live diff.
