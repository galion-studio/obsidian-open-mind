<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Non-Rust inventory — PROJECT 42 / Gaia (refresh after console-dist cutover)
Refreshed 2026-10-27 17:40 CET (+01:00), slot 261, READ-ONLY live VPS probes (systemctl, ss -ltnp, ps, curl GET on loopback). Nothing on the VPS was written, restarted or reconfigured.
Supersedes the 2026-10-20 20:50 CEST (+02:00) inventory (repo totals in §1 there are unchanged; this refresh covers the live runtime).
Standing rule (Slavik 2026-10-20 20:41 CEST +02:00): Rust only. **[RULING]** = OS package / third-party; Slavik decides.

## 1. What changed since 2026-10-20
| Item | 2026-10-20 | 2026-10-27 17:40 CET |
|---|---|---|
| console.service | Node + Vite **dev server** :8080, ~1.26 GB RSS | Node + **srvx** serving prebuilt `/opt/galion/console-dist` (1.5 MB) :8080, 3 node procs ≈ 160 MB RSS total. Still Node. Cutover 2026-10-27 17:29 CET. |
| hermes.service (legacy Python) | inactive, disabled | **masked**; hermes-rs (`hermes-gateway-aa77864`) live on :8642 |
| stray `/root/growth-20261018/bin/*.new` | running | **gone** (dir empty / no processes) |
| galion-desk, galion-one, tuner, tanga-serve, link-watch, openmind-queue | toolchain unverified | all ELF (`/opt/galion/{bin,rust/bin,tuner/bin}`, `/usr/local/bin/openmind-queue`) |
| nerve / cortex | — | nerve `openmind-nerve-bb287cc` :8670 (/state 200), cortex `gaia-cortex-bd6fbe6` :8674 |

## 2. Remaining non-Rust runtime surfaces (live)
| # | Surface | Lang | Evidence | Blocks publish? | Rust path |
|---|---|---|---|---|---|
| 1 | **console.service**: `/opt/galion/tooling/node/bin/node … srvx.mjs --static=/opt/galion/console-dist/static …/server/index.mjs` on 127.0.0.1:8080; a child runs `node --watch` (file-watch reloader in production) | JS (Node 87 MB runtime) + SSR bundle | `systemctl show console.service`, `ss -ltnp` (:8080 node), GET /console 200 | **Yes** — the public app front door is a Node process; violates Rust-only. Also `--watch` in prod is a reliability smell. | console-static (axum serving `console-dist/static`) + port the `server/index.mjs` API routes (`/api/v1/*`, `/session`, `/admin`, session-pack, pairing) into Rust. Browser-side JS bundle itself needs **[RULING]**. |
| 2 | **galion-backup.timer → `/opt/galion/rust/bin/galion-backup.sh`** | bash | `systemctl show galion-backup.service` ExecStart path; a Rust `galion-backup` ELF sits next to it unused | Yes (own code, critical path; Rust binary exists but not wired) | Point unit at the `galion-backup` ELF after a restore test (needs owner-approved unit change). Same for `galion-restore-test.sh`. |
| 3 | `/opt/galion/bin/{desk-hermes.py,pw.py,replicate-picture.py}` (+`__pycache__`) | Python | file magic `#!` in /opt/galion/bin | Soft — not running as services; check whether galion-desk shells out to them | Delete/port; confirm galion-desk does not exec them |
| 4 | `/usr/local/bin/hermes` | bash wrapper for masked legacy Hermes | `file` | No (unit masked) | Archive + remove with legacy tooling |
| 5 | cloudflared (tunnel, :20241 metrics) | Go | running | **[RULING]** — it is the public ingress | none mature; keep or boringtun+proxy |
| 6 | fail2ban-server | Python | running, 43 MB | **[RULING]** | nftables + guard-rust |
| 7 | networkd-dispatcher | Python | running, 14 MB | **[RULING]** (likely unused) | disable |
| 8 | postfix (master/qmgr/pickup, :25 loopback) | C | running | **[RULING]** | remove or lettre |
| 9 | nerve's `journalctl -f -o json` child (45 MB) | C (systemd) | ppid = openmind-nerve | No (OS tool spawned by Rust) | optional: sd-journal reader in Rust |
| 10 | OS timers: apt-daily*, dpkg-db-backup, logrotate, man-db, motd-news, sysstat*, e2scrub, tmpfiles-clean | sh/C/Python | `systemctl list-timers` | **[RULING]** | trim motd-news, man-db |

Not running but on disk (unchanged from 2026-10-20, not re-measured): `/opt/galion/tooling/hermes-agent`, `/opt/galion/hermes`, `/opt/galion/galion-light`, `crew-skills/scripts`, `from-grok-build`, `galion-src` (note: cloudflared reads its config from `galion-src/galion-infrastructure/…`, so that tree cannot simply be archived), `venv`, stt/tts/vision. GPU pod (tts_server.py, worker.py, faster-whisper) not inspected.

## 3. Publish impact (own-code blockers, in order)
1. **Console Node runtime** — the only own-code non-Rust *service* left. Cutover removed the dev server and ~1.1 GB RSS, but "publish via Rust migration" is not met until :8080 is a Rust binary. Smallest step: Rust static server for `console-dist/static` + Rust port of the `server/index.mjs` API surface (largest piece: phone pairing / session-pack auth).
2. **Backup timer still shell** — trivial switch once the Rust `galion-backup` passes a restore test; needs an owner-approved unit change.
3. **Python helpers in /opt/galion/bin** — cleanup; verify galion-desk does not call them.
4. Everything else is **[RULING]** (cloudflared, fail2ban, networkd-dispatcher, postfix, OS timers, browser JS).
Separately (not non-Rust, but publish-relevant per CALL-RECORD): live cortex still bd6fbe6, external HTTPS returns a Cloudflare 403 challenge from the VPS IP.

## Unverified
- Whether galion-desk execs the .py helpers; GPU pod; disk sizes not re-measured; external (non-loopback) console reachability.
