<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: HADES admin-safety corpus (slot 208, Gaia S5.2)

Branch `swarm/hades-safety-corpus`, base `swarm/integration-5` 0d68f46. Box only for the gate; the VPS was
read only (unit list, nerve `/state`, cortex `/health`). Nothing was installed, restarted, POSTed or written on
the VPS. Ports used: 20780 (hades-gate serve, corpus), 20781 (hades-gate serve, S3.1 red team). Both freed.
Throwaway admin keys from `hades-gate keygen` in temp dirs (removed); token strings never printed.

## Test: `crates/hades-gate/tests/real_safety_corpus.rs`

```
GALION_REAL=1 HADES_CORPUS_VPS=1 HADES_REDTEAM_PORT=20781 \
  cargo test --release -p hades-gate --test real_safety_corpus --test real_redteam -- --nocapture
```

It starts the release `hades-gate serve --port 20780`, mints a real token per case with `hades-gate mint`
for exactly `gaia_plan_bytes(steps)`, sends `POST /v1/approve` over HTTP, and compares the answer with an
in-process `check_gaia` twin (differential). Every answer must say `"executed": false`. With
`HADES_CORPUS_VPS=1` it reads the live unit and timer names over ssh (`systemctl list-units --type=service
--all`, `systemctl list-timers --all`, read-only) and sends every name as a target.

### Before: same test, gate sources of 0d68f46 (2026-10-21 20:45:27 +02:00)

```
corpus account: 55/74 refused over HTTP
corpus action: 13/13 refused over HTTP
corpus destructive: 53/66 refused over HTTP
corpus never-unroute: 1/80 refused over HTTP
corpus opaque: 68/71 refused over HTTP
corpus protected: 41/60 refused over HTTP
corpus secret: 28/38 refused over HTTP
attacks: 402 total, 394 with a minted valid token, 143 bypasses
test real_corpus_over_http ... FAILED
```

(8 cases cannot be minted at all because `mint` refuses the target or action name; they go without a token.)

### After: branch head (2026-10-21 20:45:12 +02:00, final re-run after the doc commit below)

```
REAL TOTAL WouldApply: 0 / 82                      (real_redteam, port 20781)
corpus account: 74/74 refused over HTTP
corpus action: 13/13 refused over HTTP
corpus destructive: 66/66 refused over HTTP
corpus never-unroute: 80/80 refused over HTTP
corpus opaque: 71/71 refused over HTTP
corpus protected: 60/60 refused over HTTP
corpus secret: 38/38 refused over HTTP
attacks: 402 total, 394 with a minted valid token, 0 bypasses
legit: 21/21 approved once, replay refused
vps units: 154 read, 104 refused, 50 pass the safety rule
ledger lines 619, elapsed 3.6 s
test result: ok. 1 passed
```

Live unit names that still pass the safety rule as targets (20:45 +02:00):
`cloudflared console console-screen console-setup cron display-manager dmesg e2scrub_all e2scrub_reap fstrim
gaia-cortex galion-desk galion-health galion-nerve galion-nerve-shadow galion-one hermes hermes-rs kbd
keyboard-setup ldconfig link-watch link-watch-rs logrotate man-db memory-gateway motd-news openmind-memory
openmind-queue plymouth-quit-wait plymouth-start proxmox-regenerate-snakeoil rsync rsyslog run-u11093
run-u11094 run-u11095 setvtrgb ssl-cert sysstat sysstat-collect sysstat-summary tanga-proto tuner tuner-quick
ua-auto-attach ua-timer ubuntu-advantage ubuntu-advantage-cloud-id-shim uuidd`.

The first live run (20:43 +02:00) let 66 names pass, among them `emergency`, `rescue` (a restart drops the
box to a rescue shell without sshd), `ovsdb-server` (network), `apt-daily-upgrade` (package upgrade incl.
sshd), `rc-local`, `modprobe@*`, `kmod-static-nodes`, `initrd-*` and `ua-reboot-cmds`. Those were added to the
protected/never-unroute lists and to the corpus (second pass, commit "second pass from the live VPS unit
run"), giving 104 refused / 50 passing.

## VPS read-only observations (Zagreb time)

- 20:20:37 +02:00 (VPS clock 18:20:37 UTC): 154 service units + 15 timers listed. Never-unroute roles on the
  VPS: mail `postfix`, `postfix@-`, `postfix-resolvconf`, `exim4`, `sendmail`; DNS `systemd-resolved`;
  network `systemd-networkd`, `networkd-dispatcher`, `NetworkManager`, `connman`, `ovsdb-server`; backup
  `galion-backup.service` + `galion-backup.timer`, `dpkg-db-backup` (+ timer). All are refused now; on 0d68f46
  all of them were approvable heal targets.
- Nerve `GET 127.0.0.1:8670/state`: 20:20 +02:00 HTTP 200, 36378 bytes, recent alerts galion-one mouth 503
  loading and edge 14 hostnames -> 9 closed ports; 20:32 +02:00 cycle 8, mode `dry-run`, score 61 grade D.
- Cortex is live: `gaia-cortex.service` active since 19:18:03 +02:00, binary
  `/opt/galion/rust/bin/gaia-cortex-c7f56b8`, listening 127.0.0.1:8674. `GET /health` 20:32:12 +02:00:
  `ok true, mode dry-run, nerve true, llm_enabled true, events_in 17, incidents_open 1, escalations_open 0`.
- No `/var/lib/gaia`, no hades or gaia units other than `gaia-cortex` on the VPS (HADES is not installed
  there; nothing in this slot changes that).

## Box test numbers

- `cargo test -p hades-gate -p gaia`: 92 passed, 0 failed (incl. `safety_corpus` 7/7, `safety_corpus_legit`
  4/4, `redteam` 3/3, unit tests 17/17).
- Workspace `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`
  (20:31-20:41 +02:00, at the first fix commit): 1252 passed, 0 failed, 6 ignored.
