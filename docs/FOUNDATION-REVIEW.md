# Foundation review: Galion stack ("Obsidian Open Mind, made by Galion")
Written 2026-10-18 22:15 CEST (+02:00), ws/foundation. Host: OVH VPS 51.38.137.193, Proxmox LXC, Ubuntu 24.04, clock UTC.
Disk at review: 100G ZFS subvol, 19G used (19%). Journal was 473M (now capped, 271M).

## 1. Service map
| Service | Unit | Listen | Binary / start | Config / env (secret?) | Data |
|---|---|---|---|---|---|
| openmind-memory (Rust) | openmind-memory.service, Restart=always | 127.0.0.1:8420 | /opt/galion/rust/bin/openmind-memory | /etc/openmind-memory.env (0600, API key) | /var/lib/memory-tencentdb (SQLite WAL `instances/galion/vectors.db` + jsonl/json/md files) |
| hermes-rs (Rust) | hermes-rs.service, Restart=on-failure | 127.0.0.1:8642 | /opt/galion/rust/bin/hermes-gateway | /opt/galion/hermes/.env (provider keys), config.yaml, SOUL.md | /var/lib/hermes-rs (rs-state.db, rs-spend.json); legacy /opt/galion/hermes/state.db (20M, RO) |
| link-watch-rs (Rust) | link-watch-rs.service | none (outbound probe) | /opt/galion/rust/bin/link-watch | unit env | /opt/galion/galion-light/state/link*.json |
| galion-health (new) | galion-health.service | 127.0.0.1:8660 | /opt/galion/rust/bin/galion-health | unit env | none |
| galion-desk | galion-desk.service | 127.0.0.1:8646 | /opt/galion/bin/galion-desk | - | - |
| tuner | tuner.service | 127.0.0.1:8650 | /opt/galion/tuner/bin/tuner | - | - |
| tuner-quick | transient (/run/systemd/transient) | quick tunnel -> 8650 | cloudflared --url | - | - |
| one | **no unit**; orphan in user session-10689.scope, cwd /opt/galion/galion-light | 127.0.0.1:8091, :8002 (503) | ./one/one (old binary) | galion-light/secrets | galion-light/state |
| console-ide | console.service | 127.0.0.1:8080 | node + Vite **dev** server | scripts/with-app-env.mjs | - |
| cloudflared | cloudflared.service | metrics 127.0.0.1:20241/20242 | /usr/local/bin/cloudflared | /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml, tunnel creds (secret) | - |
| sshd, fail2ban, postfix(:25 loopback), rsyslog, journald, cron (no crontab) | system | :22 public | | | |

Startup order: network-online -> openmind-memory -> hermes-rs -> link-watch-rs; galion-health independent. `one` and tuner-quick do NOT survive reboot.

Secret locations (paths only): /opt/galion/hermes/.env, /etc/openmind-memory.env, /etc/memory-gateway.env, /root/galion-secrets/* (pairing store admin-device.json, tokens, age keys), /opt/galion/galion-light/secrets/ (+ persist/github.token), cloudflared tunnel credentials JSON.

## 2. Single points of failure
1. **Single VPS, single disk pool, no offsite copy.** Backups now exist but on the same host. Loss of the LXC = loss of memory store + secrets. (Free option: periodic encrypted pull to the owner's laptop; age keys already used.)
2. **`one` (8091) is unsupervised**: orphaned process in an ssh session scope; no unit, no restart, gone on reboot or session cleanup.
3. **console runs a Vite dev server in production** (node, HMR, dev deps); heavy and fragile.
4. **cloudflared single tunnel/connector**; all public ingress dies with it (Restart covers crash, not config errors). Quick tunnel on 8650 is transient and its URL changes on restart.
5. **openmind-memory single SQLite file** with one writer; hermes depends on it (recall degrades, 2 s timeout).
6. **Secrets as plain files** on the same disk as backups; backup archives contain them (0600 root, never leave host).

## 3. Gaps found
- Logging: services use ad-hoc `eprintln!` lines (no `tracing`); formats differ per binary; journald had no limits. Fixed: journald cap; galion-health supports `GALION_LOG_FORMAT=json|kv`. Remaining: same switch in memory/hermes (growth workstream owns that code; ~10-line change in their `log.rs`/`log()`).
- Monitoring: none before; no alerting. Now galion-health (pull only). No push alert yet.
- Backups: only manual pre-switch copies in /root/galion-switch-20261018-2140. Now 6-hourly timer.
- Deploy pipeline: manual scp/tar + hand-written rollback.sh per switch. Now galion-deploy.
- Schema: memory DB had `user_version=0`, no migration table; schema applied by idempotent CREATE IF NOT EXISTS + a blind ALTER. Baseline table added (see 5).
- survival.json goes stale (cron off); no timer replacement yet.
- `.backup` dir inside memory data dir is empty (legacy gateway feature unused).

## 4. Storage risks
- Raw copies of live WAL DBs (as in earlier tgz) can be inconsistent; new backups use SQLite online backup API.
- Disk: 82G free, low risk. Journal capped at 300M; backups ~18M each x 12 = ~220M.
- hermes legacy state.db 20M grows only if Python returns.

## 5. Implemented in this workstream
- journald: /etc/systemd/journald.conf.d/50-galion.conf (SystemMaxUse=300M, KeepFree=5G, 30 days, 32M files).
- galion-health: 127.0.0.1:8660 (`/`, `/status.json`, `/healthz` 200/503). Unit galion-health.service.
- Backups: galion-backup.timer (00/06/12/18:17 UTC = 02/08/14/20:17 CEST) -> /var/backups/galion/galion-<stamp>.tar.gz, keep 12, min free 2G, flock. Restore test: galion-restore-test.sh.
- Deploy: /opt/galion/rust/bin/galion-deploy (docs/DEPLOY.md).
- Schema: `schema_migrations` table in memory vectors.db with baseline row v3; `galion-backup schema <db>` reports it.

## 6. Storage recovery steps
Persistence verified: openmind-memory restarted 22:14 CEST, schema_migrations row and data intact, health 200.
1. `systemctl stop hermes-rs openmind-memory`
2. `T=$(mktemp -d); tar -C $T -xzf /var/backups/galion/galion-<stamp>.tar.gz; cd $T/galion-*; sha256sum -c SHA256SUMS`
3. `galion-backup verify db/memory-vectors.db`
4. Move aside live DB: `mv /var/lib/memory-tencentdb/instances/galion/vectors.db{,-wal,-shm} /root/<dir>/` then `install -m644 db/memory-vectors.db /var/lib/memory-tencentdb/instances/galion/vectors.db`
5. Files: `tar -C / -xf files/files.tar var/lib/memory-tencentdb` (selective paths; never blind-extract over /etc unless intended). hermes: `install -m600 db/hermes-rs-state.db /var/lib/hermes-rs/rs-state.db`.
6. `systemctl start openmind-memory hermes-rs; curl 127.0.0.1:8660/healthz`
DB map is in db/MAP inside each archive.

## 7. Remaining, priority order
1. Offsite (free) copy of backups: encrypted with existing age key, pulled by the owner's laptop.
2. Put `one` under systemd (or replace with galion-one Rust) -- UI workstream.
3. Alerting: galion-health -> ntfy/Telegram on state change (needs owner choice of channel).
4. JSON log switch in openmind-memory/hermes-gateway (growth workstream).
5. Production build for console (static, served by a small server) instead of Vite dev.
6. survival.json refresh via systemd timer (cron is off).
7. Route galion-health behind Cloudflare Access (DNS workstream), never public without auth.
8. Code-level migrations in openmind-memory: read schema_migrations, apply numbered steps, set user_version.
