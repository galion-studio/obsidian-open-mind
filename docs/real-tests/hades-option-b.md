<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: hades-option-b (wave 4 slot 124)

All times Europe/Zagreb CEST +02:00, 2026-10-21. Box ports 19940-19942 (slot 124 range). Throwaway keys in a
temp dir; no token string or key is printed. VPS: read-only (`systemctl list-units`, `ss -ltn`, loopback GETs).

## 1. Box, release binary, HTTP + CLI (04:45:07-04:45:15)

```
GALION_REAL=1 HADES_TYPED_PORT=19940 HADES_REDTEAM_PORT=19941 HADES_CLI_PORT=19942 \
  cargo-slot test --release -p hades-gate --test real_typed --test real_redteam --test real_cli -- --nocapture --test-threads 1
```

Prose path (`real_redteam`, POST /v1/approve on :19941):
```
REAL shell: 0 WouldApply / 25   file: 0 / 19   access: 0 / 14   allowverb: 0 / 10   unicode: 0 / 6   traversal: 0 / 8
REAL TOTAL WouldApply: 0 / 82
REAL legit not approved: 0 / 9
```
Typed path (`real_typed`, POST /v1/approve-typed on :19940, each attack raw and header-wrapped, valid token):
```
REAL typed refusal code typed-plan: 150
REAL typed refusal code malformed: 14
REAL typed TOTAL WouldApply: 0 / 164
REAL typed legit not approved: 0 / 9      (and every replay -> HTTP 403)
REAL check-typed dry-run exit=0 {"decision":"DryRun",...}
REAL check-typed token   exit=0 {"approved":{"action":"heal-apply","target":"galion-tuner","plan_sha256":"5e1e5b39…",...},"decision":"WouldApply","executed":false}
REAL check-typed replay  exit=2 {"code":"replayed","decision":"Refused",...}
REAL check-typed prose   exit=2 {"code":"typed-plan","reason":"typed plan must start with the \"hades-plan/v1\" line"}
```
`real_cli` (existing, port now overridable via `HADES_CLI_PORT`): 1 passed (2.12 s). Afterwards `ss -ltn` showed
no listener on 19940-19949.

## 2. VPS, live read-only data (04:46-04:52)

Probes: HADES on the VPS = **not deployed** (no listener, no `hades*` unit, no binary in /usr/local/bin).
Health GETs: 8420 200 (openmind-memory), 8646 200, 8650 200 (tuner), 8091 200, 8660 404 (galion-health has no /health).
`systemctl is-active galion-tuner` -> inactive (unit does not exist; the real unit is `tuner`).

Nerve `127.0.0.1:8670/state` (cortex not deployed yet, so the nerve is the observer):
`ts=2026-10-21T04:46:05+02:00 mode=live cycle=6503 started=2026-10-18T22:35:04+02:00`; healthy: openmind-memory,
hermes-rs, link-watch-rs, galion-desk, galion-one, console, galion-health, tuner-quick, lxc-host, galion-nerve;
degrading: tuner, galion-one-mouth, cloudflared; absent: voice-worker (galion-voice), gaia.

```
GALION_REAL=1 HADES_VPS_SSH=slavik-vps cargo-slot test --release -p hades-gate --test real_vps_inventory -- --nocapture
```
(04:52:45; the test itself runs `systemctl list-units` and a loopback curl GET of /state over ssh)
```
REAL vps nerve ts="2026-10-21T04:52:35+02:00" mode="live" cycle=6516
REAL vps units=152 deny-list accepts=142 refuses=10
REAL vps refused: initrd-switch-root iptables nftables ssh systemd-fsck-root systemd-logind systemd-user-sessions ufw user-runtime-dir@0 user@0
REAL vps security units present=[firewalld, fail2ban, auditd, apparmor, nftables, iptables, ufw, ssh, cron] deny-list lets through=[firewalld, fail2ban, auditd, apparmor, cron]
REAL nerve units not installed on the VPS: ["galion-voice"]
REAL inventory (10): cloudflared console galion-desk galion-health galion-one hermes-rs link-watch-rs openmind-memory tuner tuner-quick
REAL typed restart plans for real units: 10/10
REAL inventory refuses deny-list leaks: 5/5
REAL corpus legit plans naming a unit not on the VPS: 5/9   (all: galion-tuner)
test result: ok. 1 passed
```
Same test from box copies (`HADES_VPS_UNITS_FILE` / `HADES_NERVE_STATE_FILE`, taken 04:46:21): same numbers.

## 3. Gates

* hades-gate: 61 passed / 0 failed (14 suites, debug); workspace (`--exclude openmind-sandbox --exclude cgroup-detect`): 512 / 0 (122 suites).
* clippy `--all-targets -D warnings`: 0 (hades-gate and workspace); `cargo fmt --all --check`: clean; `scripts/no-python.sh`: ok.
* hades-gate release: x86_64-unknown-linux-musl and aarch64-unknown-linux-musl (rust-lld) build; aarch64-linux-android and armv7-unknown-linux-musleabihf check.

## Slip (honest)

At 04:46 one probe command on the VPS contained a stray redirect that created and then immediately removed
`/tmp/nonexist-check` (a list of unit names, no secrets) in the same command. No other write happened on the
VPS; no service, unit or config was touched; /opt/galion/openmind was not touched.
