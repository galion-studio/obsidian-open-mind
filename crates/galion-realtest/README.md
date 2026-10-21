<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# galion-realtest

Platform-wide real-life test harness (Rust, std + serde_json only).

## `galion-realtest stack`
1. Locates release binaries (`--bin-dir`, else `$CARGO_TARGET_DIR/release`, else `target/release`) or builds them with `--build` (`nice -n 10 cargo build --release --locked -j 4`).
2. Writes fixture configs under `<out>/fixture/` (pairing stores, fixture web roots, a throwaway memory key in an env file, a `boot.toml`) and starts a fixture upstream stub (fake Ollama/Hermes; every model or TTS call gets 503, so nothing is ever paid for).
3. Boots the whole stack under the stack's own supervisor (`galion-boot supervise`) in its own process group, with a clean environment, on `127.0.0.1:BASE..BASE+51` (default 19500).
4. Waits for health, then runs the scenarios:

| Scenario | What it checks |
|---|---|
| stack-boot | supervisor up, every service healthy, readiness time per service |
| one-pairing | unpaired is refused (`/me`, `/session`, `/api/v1/analytics`, `/files`), wrong code refused, right code (any case) issues an HttpOnly + SameSite=Strict cookie that works, lockout after 5 wrong codes refuses even the right code, store holds hashes only |
| one-fail-closed-missing-store / -empty-store | One with no device store or `{}` must stay closed |
| memory-save-search | openmind-memory: bearer auth, `/capture` saves, `/search/conversations` finds it, session filter, `/search/memories`, v2 service-id rule, data on disk |
| nerve-ingest | `POST /signal` -> 202, visible in `/signals` (skipped when no nerve binary exists, as at 13a296c; `--with-nerve` to run it) |
| tuner | exact health contract, HEAD, page and assets, 405, 404, no traversal |
| link-watch | writes link.json from the stub, Zagreb `at_local`, notices Hermes going down and up, logs the change event |
| desk | health, snapshot, note, voice switch, input sanitising, 404 |
| voice-relay-contract | persona-plex-relay scaffold: one Zagreb stamp with no config, plan never prints the secret values, exit codes |
| guardian-tick-heal | guardian-tick green, tuner SIGKILLed, guardian-tick reports it with the "operator may restart tuner" suggestion, the supervisor restarts it (new pid), guardian-tick green again, JSONL stamps are Zagreb |
| heal-request | the healer channel (`<supervisor_dir>/restart/<name>`) restarts exactly that service |
| teardown | process group gone, every port free |

5. Writes `junit.xml` and `report.md` (Zagreb timestamps) to `--out` (default `target/realtest/stack-<stamp>`); service logs stay in `<out>/fixture/logs/`.

`tests/real/known-failures.txt` lists `scenario/check` ids that are known to fail. They are reported as KNOWN-FAIL and do not fail the exit code. A listed check that passes is flagged so the list can be pruned.

## `galion-realtest live` (READ-ONLY)
Over `ssh <host> sh -s` (default host `slavik-vps`) it runs a fixed script: `curl -sS -m 6 -D - URL` GETs against loopback health URLs, `systemctl is-active` and `ss -ltnH`. From the box it GETs `https://one.galion.app/{health,,me,session,api/v1/analytics,tanga/}`. Then it compares the results with the expected contract. It never POSTs, never writes a file on the VPS and never restarts anything (a unit test pins the script). Bodies that could hold private data are recorded as a byte count only.

Exit: 0 = no unexpected failure, 1 = a check failed, 2 = usage/setup error.

## Ports of the old `scripts/real/*.sh` (`tests/real_sh_*.rs`)
Rust-only rule (2026-10-20 20:41 CEST +02:00): the shell real-life scripts are being replaced by gated Rust integration tests in this crate. Each `tests/real_sh_<name>.rs` builds the release binaries it needs (`nice -n 10 cargo build --release -j 2`), starts them on loopback, probes them with real requests, prints PASS/FAIL lines with Zagreb stamps and tears everything down (also on panic). Shared helpers are in `src/script.rs`.

| Env | Meaning |
|---|---|
| `GALION_REAL=1` | run at all (otherwise each port prints SKIP and passes) |
| `GALION_REAL_PORT_BASE` | move the port block (each file documents its default block; all offsets are < 10) |
| `GALION_REAL_SKIP_BUILD=1` | use already built binaries |
| `GALION_REAL_JOBS` | `cargo build -j` (default 2) |
| `GALION_REAL_KEEP=1` | keep the scratch dir for inspection |

```
GALION_REAL=1 GALION_REAL_PORT_BASE=19820 cargo test -p galion-realtest --test real_sh_hades_approval -- --nocapture
```

Ported on swarm/realscript-port-1: common, desk-linkwatch, docs-sync, edge-boot, gaia-hades-tokens (+ the gaia-live wrapper), gaia-poseidon, hades-approval, hermes-tools. Evidence: `docs/real-tests/realscript-port-1.md`.
