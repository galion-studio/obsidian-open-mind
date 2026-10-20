<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: cortex-s2-a (T2.1 `sshd`, T2.4 `rustsec`)

- Written: 2026-10-20 23:30 CEST (+02:00). Branch `swarm/cortex-s2-a`, base `swarm/cortex-sensor` e536446 (T1.2 Sensor trait).
- Slot 55, ports 19550-19559 only. Box only; the VPS got read-only `journalctl`/`stat`/`sshd -T`/`systemctl is-active` reads (counts only, no addresses copied).

## What was built

| sensor | file | on when | reads | findings (B33 source) |
|---|---|---|---|---|
| `sshd` (T2.1) | crates/openmind-nerve/src/sensors/sshd.rs | `NERVE_SSHD=1` | `Input.journal` lines of `ssh.service`, `sshd.service`, `sshd@*.service` (`NERVE_SSHD_UNITS`); live only: `stat` size of `NERVE_SSHD_AUTH_LOG` (default /var/log/auth.log) | `sshd-accepted-password` (crit), `sshd-accepted-publickey-other` (high/warn), `sshd-invalid-user`, `sshd-max-auth-attempts` (warn): vulnerability; `sshd-credential-flood` + signal `ssh-flood`: ddos; `sshd-auth-log-growth`: bleeding. Source `auth.log` (B41) |
| `rustsec` (T2.4) | crates/openmind-nerve/src/sensors/rustsec.rs | `NERVE_RUSTSEC_LOCK=<Cargo.lock>` | live only: the lock (refresh on mtime change or every `NERVE_RUSTSEC_EVERY_S`, 3600), embedded offline snapshot or `NERVE_RUSTSEC_DB` | `rustsec-<pkg>-<ver>` vulnerability: CVSS3 >= 9 crit, other vuln high, unsound warn, unmaintained/notice info. Source `rustsec` |

B41 thresholds (docs/cortex.md section 4) are copied verbatim (300 s window; tests cover every edge). Titles, evidence
and events carry counts, package names, versions and advisory ids only: addresses are hashed in memory to count
distinct sources and dropped; account names are never kept (tests assert no fixture address/name in the report).
Registry: two lines in `sensors/mod.rs` (`pub mod` x2, `add_opt` x2). engine.rs / cortex.rs / config.rs untouched.

## Gates (2026-10-20 23:19-23:25 CEST, +02:00)

| gate | result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo test --workspace` | 980 passed, 0 failed, 4 ignored (before the data-only `preauth_other` key; openmind-nerve re-run after it: 97 passed (lib 75 + integration 22)) |
| `cargo test -p openmind-nerve --examples` (CVSS v3 calculator) | 1 passed |
| `scripts/no-python.sh` | `no-python: ok` |
| `scripts/spdx-scan.sh` | scanned 365, missing 0 |
| `cargo check -p openmind-nerve --target` x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, aarch64-linux-android, armv7-unknown-linux-musleabihf | ok (armv7-gnueabihf std not installed on the box) |
| T1.2 golden replay `tests/cortex_replay.rs` | unchanged (both sensors are off by default) |

## Real test: `GALION_REAL=1 cargo test --release -p openmind-nerve --test real_cortex_s2a -- --nocapture`

Run 2026-10-20 23:24:45 -> 23:25:17 CEST (+02:00), 1 passed in 20.6 s. The test builds, starts, probes and kills
the release binary itself; `ss -ltn | grep :1955` afterwards: nothing left.

```
replay cortex-sshd: exit Some(1) score 67 grade "D"; flood: "ssh credential flood: 400 failed password(s) from 50 source(s) in 300 s (rule failed_password)"; vulns ["sshd-accepted-publickey-other", "sshd-invalid-user"]
serve :19550 cycle 1: rustsec ["rustsec-smallvec-1.6.0", "rustsec-time-0.1.45"]; event {"ts":"2026-10-20T23:24:57+02:00","source":"rustsec","kind":"vulnerability","severity":"crit","data":{"findings":2,"ids":["rustsec-smallvec-1.6.0","rustsec-time-0.1.45"],"top":{...,"id":"rustsec-smallvec-1.6.0","title":"smallvec 1.6.0 in Cargo.lock: 1 RustSec advisory (RUSTSEC-2021-0003)"}}}; sensors listeners=2us scanners=0us edge=0us components=0us ddos=0us sshd=4us rustsec=1829us; cycle_ms 24; rss 3952 KiB
serve :19550 cycle 3: +300 KiB -> "auth log grew 300 KiB in 300 s (disk bleed)" ["warn"]
serve :19550 cycle 4: +1.1 MiB -> "auth log grew 1400 KiB in 300 s (disk bleed)" ["error"], event {...,"kind":"bleeding","severity":"high","source":"auth.log","ts":"2026-10-20T23:25:12+02:00"}
serve :19550 cycle 5: lock fixed + log rotated -> cleared; sensors listeners=3us scanners=0us edge=0us components=0us ddos=15us sshd=5us rustsec=1790us; cycle_ms 24; rss 4184 KiB
serve :19552 workspace lock: ["critical"] "zip 7.4.0 in Cargo.lock: 3 RustSec advisories (RUSTSEC-2026-0367, RUSTSEC-2026-0368, RUSTSEC-2026-0369)"
serve :19552 workspace lock: ["info"] "paste 1.0.15 in Cargo.lock: 1 RustSec advisory (RUSTSEC-2024-0436)"
serve :19552 cycle 1: sensors ... rustsec=1905us; rss 4084 KiB
```

Budget: whole cycle 24 ms (< 50), RSS 3.9-4.2 MiB (< 8). `sshd` 4-5 us per cycle; `rustsec` ~1.8 ms only on a
refresh cycle (first cycle, lock mtime change, hourly), a `stat` otherwise. Release binary +147 744 bytes (snapshot 97 216).

Not covered on the box: live sshd journal lines (the box has no journald: `journalctl` says "No journal files were
found"). The journal path is covered by the replay fixture through the real binary and by unit tests.

## Read-only VPS probes (counts only)

2026-10-20 23:23:43 / 23:24:02 / 23:24:15 CEST (+02:00), `journalctl -u ssh.service` on 51.38.137.193:

| window | count |
|---|---|
| last 300 s | 27 lines; `Accepted publickey for root` 7; everything else 0; auth.log 9 197 350 bytes, +0 in 10 s |
| last 24 h, `Failed password` | **0** (incl. `for invalid user`: 0) |
| last 24 h, `Invalid user` | 445 (max 17 per 5 min: under the 30 warn line) |
| last 24 h, `Accepted publickey for root` / other / `Accepted password` | 769 / 0 / 0 |
| last 24 h, `maximum authentication attempts` | 5 (each would be a B41 warn) |
| last 24 h, `[preauth]` lines | 1539 (max 43 per 5 min): closed/disconnected invalid user 437, authenticating user 448, timeout 89 |
| `sshd -T` | `passwordauthentication no`, `kbdinteractiveauthentication no`, `maxauthtries 3` |
| unit | sshd logs as `_SYSTEMD_UNIT=ssh.service` (matches the default `NERVE_SSHD_UNITS`); ssh.service and galion-nerve.service active |

So the live VPS today would raise: occasional `sshd-max-auth-attempts` warn and nothing else. The B41 ddos rule keys
on `Failed password`, which this sshd no longer logs (password auth is off), so the ssh flood signal is quiet even
while ~1500 preauth disconnects a day happen. `preauth_other` is now counted as data only (evidence line), no threshold.

## Findings

1. **zip 7.4.0 in the workspace Cargo.lock: 3 advisories, worst CVSS3 9.1** (RUSTSEC-2026-0367 7.3, -0368 no v3
   score, -0369 9.1; patched `>= 9.0.0`). Path: `galion-voice-cpu` -> `candle-core` 0.9.2 / `candle-nn` -> `zip`.
   Only built with galion-voice-cpu's optional candle feature (`cargo tree -i zip` finds nothing without
   `--all-features`). The sensor judges the lock, not the feature set, so it reports it.
2. paste 1.0.15 unmaintained (RUSTSEC-2024-0436), via candle's `gemm`: info only.
3. B41 ddos rule vs today's sshd config (above): quiet by construction.
4. The live engine collects journal lines only for component units; `ssh.service` is not one, so on a live nerve the
   `sshd` sensor sees nothing until the T2.0 merger adds the ssh unit(s) to the journald collection (collect.rs) or a
   component spec names `ssh.service`. Not done here (shared file).

## Owner decisions (Slavik)

- **D1 ssh flood rule with password auth off.** (a) keep B41 verbatim (ddos quiet; today's behaviour); (b) add a
  `preauth_other` threshold (e.g. warn >= 100 / high >= 300 per 300 s; VPS max today 43); (c) count
  `Invalid user` + preauth as the flood metric. Evidence: VPS table above. B41 says thresholds may only be tightened
  from real 300 s windows (T3.1).
- **D2 zip 7.4.0 via candle (CVSS 9.1).** (a) bump candle when a release uses zip >= 9; (b) accept while the candle
  feature is off in deployed builds; (c) teach the sensor a per-binary lock (`cargo tree --edges normal` of the
  deployed feature set) so feature-gated crates are not reported.
- **D3 CC-BY-4.0 advisories.** 22 GHSA-imported advisories are excluded (CC-BY-4.0 not on the allowlist). Include
  them with attribution (url field kept) or keep excluded.
- **D4 defaults.** Both sensors are opt-in (`NERVE_SSHD=1`, `NERVE_RUSTSEC_LOCK=`), so goldens and other sensors'
  tests are unchanged. Turning them on in the live unit env (with the deployed SHA's Cargo.lock path) is part of the
  galion-nerve cutover decision.
- **D5 severity of informational advisories.** unsound = warn (scored 4), unmaintained/notice = info (not scored).
  Raise unmaintained to warn?
- **D6 snapshot refresh cadence.** The snapshot is a dated commit (e6f94f3, 2026-10-20 21:24 +02:00); regenerating it
  needs one read-only `git clone` from GitHub (no account, no spend). Who refreshes it and how often.
