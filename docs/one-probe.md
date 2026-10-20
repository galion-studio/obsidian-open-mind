<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# One probe

- DESK-ID: openmind.ONE-PROBE
- Stamp: 2026-10-20T10:44:17+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: crates/galion-one
- Result: PASS

## What was wrong

A spare process with `ONE_BIND=127.0.0.1:18091` and `ONE_MOUTH_BIND=127.0.0.1:18002` still opened client connections to `127.0.0.1:8091` and the mouth client port 8002. `docs/reallife-one-2.md` recorded that. `ONE_MOUTH_BIND` moved only the listen address. The self probe used a fixed port. The mouth client used `mouth_port`.

## What changed

`one_probe_target` returns the listen address in `Settings.bind`. `mouth_client_target` returns `GALION_MOUTH` when that value parses, otherwise the mouth listen address in `Settings.mouth_bind`. `sample` dials those for the `one` and `voice` probes. `mouth_addr` uses the same mouth client target. No listener was added.

`mouth_port` is still 8002. A loopback client on that port stays local. A client that matches the mouth listen address stays local too, so a moved mouth does not dial the live port and does not call its own `/synthesize`.

## Test

`probe_and_mouth_client_follow_forced_bind_and_mouth` calls `resolve` on a missing config file with `ONE_BIND=127.0.0.1:18091` and `ONE_MOUTH_BIND=127.0.0.1:18002`. The probe target is the forced bind. The mouth client target is the forced mouth. Neither target is `127.0.0.1:8091` or port 8002. The test parses addresses only. It does not listen.

## Checks

`cargo fmt -p galion-one -- --check`, `cargo clippy -j 1 --offline -p galion-one --all-targets -- -D warnings`, and `cargo test -j 1 --offline -p galion-one` exited 0. Target dir was `/tmp/om-oneprobe`. `Cargo.lock` matched `/tmp/om-lock-ONEPROBE-before` and was not copied back. `git checkout` was not used.

11 tests passed.

pid 1373612 was not signaled and was not restarted. galion-one was not started. This work did not bind 8091, 8002, 18091, or 18002. No other crate was edited. Nothing was committed. Nothing was pushed. systemctl was not used.

The stamp offset is numeric. It is not a trailing UTC designator.

## Port onto the portable galion-one (swarm/one-probe, 2026-10-20T19:05+02:00)

The text above records the VPS fix (2f5a730). That code sat on the VPS-only
`Settings`/`openmind-config` layer (B06), so it did not apply to GitHub main's
portable galion-one. The behaviour is now ported onto main's env-only design in
`crates/galion-one/src/probe.rs`; the full write-up, the list of dropped
VPS/live features and the cutover differences are in
[one-probe-cutover.md](one-probe-cutover.md). Real-life evidence:
[real-tests/one-probe.md](real-tests/one-probe.md).
