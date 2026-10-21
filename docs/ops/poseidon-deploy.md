<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Deploy record: `openmind-queue` (POSEIDON) on the VPS

- Approved: Slavik via Mate, 2026-10-21 08:23 +02:00 (t172, item 4: "161 deploy openmind-queue (POSEIDON) on VPS").
- Source: `swarm/poseidon-refusal-persist` eec2d9914df0706810aae75d2cba62da8ea90950 (see `docs/poseidon.md`,
  `docs/real-tests/poseidon-refusal-persist.md`).
- Done: 2026-10-21 08:26-08:29 +02:00 (Europe/Zagreb). The VPS address is kept out of the repo.
- No secrets, no keys, no paid API: the service has none and needs none. Loopback only; no tunnel rule, no DNS.

## Build (box)

`cargo-slot build --release -j 2 --locked --target x86_64-unknown-linux-musl -p openmind-queue -p galion-deploy`
(`CARGO_TARGET_DIR=/workspace/swarm/target/poseidon-deploy`, removed afterwards).

| Binary | Size | sha256 |
| --- | --- | --- |
| `openmind-queue` 0.1.0, static-pie, stripped | 889 632 B | `8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d` |
| `galion-deploy` (rebuilt only to compare) | 639 664 B | `2a60e1ac632e27d9d918102d5fb02e1bafe35d4a48ead734f403f650de07464d` = the installed `/opt/galion/rust/bin/galion-deploy` |

## What was installed (VPS)

| Item | Value |
| --- | --- |
| system user | `openmind-queue` (uid 996, gid 989, `useradd --system --no-create-home --shell /usr/sbin/nologin --user-group`) |
| data dir | `/var/lib/openmind-queue`, 0700 `openmind-queue:openmind-queue` (`queue.log`, `queue.lock`, `refusals.dat`) |
| binary | `/usr/local/bin/openmind-queue`, 0755 root, sha256 as above (checked after scp and after install) |
| unit | `/etc/systemd/system/openmind-queue.service`, enabled (`multi-user.target.wants`) |
| bind | `127.0.0.1:19120` (the crate default; 1910x-1919x were free in `ss -ltn` before) |

Unit:

```
[Unit]
Description=openmind-queue (POSEIDON) work queue, 127.0.0.1:19120
After=network.target

[Service]
Type=simple
User=openmind-queue
Group=openmind-queue
ExecStart=/usr/local/bin/openmind-queue serve --bind 127.0.0.1:19120 --data /var/lib/openmind-queue
Restart=on-failure
RestartSec=2
UMask=0077
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/openmind-queue
PrivateTmp=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictSUIDSGID=yes
LockPersonality=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
MemoryMax=128M

[Install]
WantedBy=multi-user.target
```

Install flow (galion-deploy first install, F5): user + data dir + unit, `systemctl daemon-reload`, `systemctl enable`
(not started), then
`SKIP_DATA_BACKUP=1 /opt/galion/rust/bin/galion-deploy openmind-queue /var/tmp/openmind-queue-eec2d99 http://127.0.0.1:19120/healthz`
-> `first install ... backup=none sha_new=8a7f7faaaa86`, `deployed` at 08:27:28 +02:00, exit 0. `SKIP_DATA_BACKUP=1`
because a first install has no data and the system-wide galion-backup run is not part of this change. The staged file
in `/var/tmp` was removed.

## Verification (2026-10-21 08:27:41-08:29 +02:00)

| Check | Result |
| --- | --- |
| `GET /healthz` | 200 `{"function":"POSEIDON","ok":true,"service":"openmind-queue","version":"0.1.0"}` |
| listener | only `127.0.0.1:19120` (`ss -ltn`) |
| refusals | agent `deploy-check` quota 1/1: 4 submits -> `201 429 429 429`; agent `deploy-check-b` cap 5, cost 9 -> `402` |
| `/stats` before kill | `quota_refusals 3`, `budget_refusals 1`, `refusals_sidecar {bad_slots 0, fresh true, seq 4, writes 4, write_errors 0}` |
| real `kill -9` | 08:27:52: `kill -9` MainPID 17463 -> systemd `code=killed, status=9/KILL`, `Scheduled restart job, restart counter is at 1`, new pid 17979 healthy 3 s later, `NRestarts=1` |
| `/stats` after restart | `quota_refusals 3`, `budget_refusals 1` (preserved), `refusals_sidecar {restored true, restored_seq 4, bad_slots 0}`, store `replayed 3`, ready job 1 kept |
| test cleanup | the one test job leased + acked (cost 0), cap reset to null, quota reset to 100000/1000. The counters stay (they only grow) |
| nerve `127.0.0.1:8670/state` | 200, cycle 6946-6947, mode live, 11 healthy / 2 degrading (galion-one-mouth, cloudflared) / 2 absent (voice-worker, gaia): same as before the deploy |
| other services | hermes-rs active, `:8642/health` 200; no other unit touched. `systemctl --failed` = the 3 pre-existing `run-u1109x` transient units |

## Rollback

```
systemctl disable --now openmind-queue.service
rm /etc/systemd/system/openmind-queue.service
systemctl daemon-reload
# optional full removal (data is test-only today):
rm /usr/local/bin/openmind-queue
rm -r /var/lib/openmind-queue
userdel openmind-queue
```

Nothing else depends on it yet (no nerve entry, no tunnel rule, no client wired).

## Left for the owner

1. The nerve does not watch `openmind-queue` yet (Gaia POSEIDON still maps to `lxc-host`); adding it is a nerve config change.
2. Gaia POSEIDON adapter (`swarm/gaia-poseidon`, `QueueAdapter`) is not deployed; when it is, point it at `127.0.0.1:19120`.
3. No authentication: it must stay loopback-only. A LAN bind needs a token layer first.
4. Optional: `--providers` (openmind-provider registry) is not set; VRAM devices are empty (gap 1500 GB, numbers only).
