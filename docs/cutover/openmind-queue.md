<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: openmind-queue / POSEIDON (127.0.0.1:19120)

- Status: **NEW UNIT, live since 2026-10-21 08:27:54 CEST (+02:00)** (first install, not a replacement; index [README.md](README.md) §2 C7).
- **Approved: Yes**, Slavik via Mate t172 2026-10-21 08:23:38 (item 4, "deploy openmind-queue (POSEIDON) on VPS") and t175 (item 7).
  Record: `swarm/poseidon-deploy@d2c5cf192588df81cf5dbd795f9a64015e8ac7f4:docs/ops/poseidon-deploy.md`; CALL-RECORD 08:27–08:29.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from the record and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/openmind-queue.service` (896 B, sha256 `02f6fee4dca0e84e8ee9c4cec124b7f3a602bece36f1529ee7e0447d9593de51`, written 08:27:19), enabled, active, no drop-ins, `User=openmind-queue` (system user), `Restart=on-failure` 2 s, `MemoryMax=128M`, `ProtectSystem=strict`, empty capability set |
| ExecStart | `/usr/local/bin/openmind-queue serve --bind 127.0.0.1:19120 --data /var/lib/openmind-queue` |
| exe | `/usr/local/bin/openmind-queue` 0.1.0, 889,632 B, sha256 `8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d`, RSS 0.6 MB |
| source | `swarm/poseidon-refusal-persist` eec2d9914df0706810aae75d2cba62da8ea90950, x86_64 musl static |
| data | `/var/lib/openmind-queue` 0700 (`queue.log`, `queue.lock`, `refusals.dat`) |
| restarts | NRestarts 1 = the planned `kill -9` test at 08:27:52 (record); none since |
| listener | only `127.0.0.1:19120`; no tunnel rule, no DNS, no authentication (must stay loopback until slot 205 poseidon-auth lands) |

## 2. Verify (on the VPS, GET only)

```
systemctl show openmind-queue -p ActiveState -p NRestarts
sha256sum /usr/local/bin/openmind-queue                                    # 8a7f7faa…ec8d
curl -s -m 5 http://127.0.0.1:19120/healthz   # 200 {"function":"POSEIDON","ok":true,"service":"openmind-queue","version":"0.1.0"}
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:19120/stats   # 200
ss -ltn | grep 19120                                                       # 127.0.0.1 only
```

At 17:22–17:23: active, NRestarts 1, `/healthz` 200 (117 B), `/stats` 200 (1,117 B) with `ready 0`, `leased 0`, `replayed 3`,
`quota_refusals 3`, `budget_refusals 1`. These are the deploy-test counters, still preserved, so no new work has used the queue since 08:29.

## 3. Rollback (on the VPS, as root; from the record)

```
systemctl disable --now openmind-queue.service
rm /etc/systemd/system/openmind-queue.service
systemctl daemon-reload
# optional full removal (data is test-only today):
rm /usr/local/bin/openmind-queue
rm -r /var/lib/openmind-queue
userdel openmind-queue
```

Nothing depends on it yet: no nerve spec, no client wired, no tunnel rule.

## 4. Open points (from the record, still open at 17:23)

- The nerve does not watch `openmind-queue` (no component in `/state` at 17:22). Adding it is a nerve config change (plans/nerve.md T2.3).
- No authentication: it stays loopback-only. Slot 205 `poseidon-auth` (queued) builds the token layer; slot 206 `poseidon-crash-soak` covers crash/soak.
- The Gaia POSEIDON adapter (`swarm/gaia-poseidon`) is not deployed.
