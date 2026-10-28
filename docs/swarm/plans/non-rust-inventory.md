<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/plans/non-rust-inventory.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Non-Rust inventory — PROJECT 42 / Gaia
Generated 2026-10-20 ~20:50 CEST, read-only. Repo: wt/integration-1 @ 8fd26f6. VPS: <vps>.
Standing rule (Slavik 20:41 CEST): Rust only, no exceptions. Items marked **[RULING]** are OS packages or third-party software and need Slavik to decide whether "no exceptions" covers them.

## 1. Repo totals (rg --files + wc -l; .github is hidden from rg and was counted separately)
| Lang | Files | Lines |
|---|---|---|
| Rust (.rs, for reference) | 373 | 106,943 |
| sh/bash | 55 | 5,157 |
| TSX | 1 | 2,758 |
| TS | 8 | 1,262 |
| JS | 5 | 430 |
| HTML | 3 | 342 |
| CSS | 3 | 236 |
| CI YAML (.github/workflows ×6 + dependabot) | 7 | ~581 (workflows only) |
| Other YAML (arena homes ×4, cloudflared config) | 5 | 184 |
| systemd .service/.timer/.conf | 21/3/6 | 526/33/73 |
| SQL fixture | 1 | 19 |
| Python | 0 | 0 |
| Dockerfile / Makefile | 0 | 0 |

### By directory
| Dir | Lang | Files | Lines | What it does |
|---|---|---|---|---|
| scripts/real | sh | 23 | 2,578 | real-environment test drivers |
| scripts | sh | 11 | 1,440 | build, release and dev helpers |
| contrib/grokbot/workflows/vps/bin | sh | 10 | 647 | VPS ops workflow scripts |
| deploy (+deploy/nerve) | sh | 5 | 171 | deploy/install scripts |
| edge/openmind-node/install | sh | 2 | 168 | edge node installer |
| stack/kms/galion-light/guard | sh | 1 | 66 | guard script (guard-rust in progress) |
| scripts/voice | sh | 1 | 41 | voice helper (voice-rust) |
| crates/openmind-memory, crates/hermes-rs/tests/fixtures | sh | 2 | 46 | test fixture and helper scripts |
| stack/research-agent/console-ide/src/components | TSX | 1 | 2,758 | console IDE UI (a single large component) |
| stack/research-agent/console-ide/src/{lib,routes/api/hermes} | TS | 3 | 858 | console client lib and Hermes API routes |
| stack/kms/console-ide/src/{lib,routes/api/v1} | TS | 5 | 404 | KMS console lib and API routes |
| apps/phone-voice | JS+HTML | 3 | 255 | browser phone-voice client |
| crates/tuner/www (+test) | JS/HTML/CSS | 5 | 301 | tuner web UI |
| release/site, brand | HTML/CSS | 3 | 452 | static release site and brand CSS |
| .github/workflows | YAML | 6 | 581 | CI: launch-gates, realtest, release, sbom, scorecard, self-eval **[RULING]** |
| deploy/** | systemd units | 30 | 632 | unit config (declarative, not code) **[RULING]** |

## 2. VPS — running, non-Rust
| Item | Lang | What | State | Size | Rust replacement | Diff | OS/3rd |
|---|---|---|---|---|---|---|---|
| console.service (node + vite dev :8080, with-app-env.mjs) | JS/TS (Node) | console IDE served by a dev server | running, ~1.26 GB RSS | console-ide 340 MB | **console-static** (axum serving a prebuilt bundle); later a Rust UI (Leptos/Dioxus) for the TSX | M (static) / H (full UI) | own code |
| fail2ban-server | Python | SSH ban daemon | running, 43 MB | OS pkg | nftables rules + small Rust journald watcher; **guard-rust** could absorb this | M | **[RULING]** |
| networkd-dispatcher | Python | networkd hook runner | running, 16 MB | OS pkg | disable it (likely unused) or a tiny Rust D-Bus listener | L | **[RULING]** |
| cloudflared ×2 (config.yml tunnel; tuner-quick → :8650) | Go | Cloudflare tunnels | running | binary | no mature Rust equivalent (would need our own QUIC client); the alternative is the WireGuard-based boringtun plus a reverse proxy | H | **[RULING]** |
| postfix (master/qmgr/pickup) | C | local MTA | running | OS pkg | remove it, or send mail from Rust with lettre | L | **[RULING]** |
| systemd, journald, resolved, networkd, logind, dbus, sshd, cron, rsyslog, agetty | C | base OS | running | OS | none sensible | — | **[RULING]** |
| /opt/galion/bin/galion-desk, galion-one, tuner, tanga-serve, link-watch | ELF | services | running | — | toolchain not verified; assumed Rust, confirm | — | — |
| /root/growth-20261018/bin/{hermes-gateway,openmind-memory}.new | ELF | stray copies running outside /opt | running | — | Rust, but they need cleanup | — | — |

## 3. VPS — timers, cron and the scripts they call
| Item | Lang | What | State | Size | Replacement | Diff | OS/3rd |
|---|---|---|---|---|---|---|---|
| galion-backup.timer → /opt/galion/rust/bin/galion-backup.sh | bash | tar-based backup (12 tar calls); it sits in rust/bin but is a shell script | active timer | 59 lines | `galion-backup` crate (tar + zstd crates) | L | own code |
| /usr/local/bin/galion-offsite-serve | unknown (not inspected) | offsite backup serving | not inspected | — | needs a check; fold into galion-backup | L–M | own code |
| apt-daily*, dpkg-db-backup, logrotate, man-db, sysstat, e2scrub, fstrim, motd-news, ua-timer, systemd-sysupdate*, tmpfiles-clean; cron.d e2scrub/sysstat; cron.daily apt-compat/dpkg/logrotate/man-db/sysstat | sh/C/Python | OS maintenance | active | OS | none, or trim the unneeded ones (motd-news, ua-timer, man-db) | — | **[RULING]** |
| root crontab | — | — | empty | — | — | — | — |

## 4. VPS — present on disk but not running
| Item | Lang | What | State | Size | Replacement | Diff | OS/3rd |
|---|---|---|---|---|---|---|---|
| hermes.service → /usr/local/bin/hermes (bash wrapper) + /opt/galion/tooling/hermes-agent | Python (~2k .py files under tooling) | legacy Hermes agent | **inactive, disabled**; replaced by hermes-rs | tooling 458 MB | already done by hermes-gateway (hermes-rs). Archive, then delete | L (removal) | own/3rd |
| /opt/galion/hermes | data + Python | Hermes home/state (config, auth, cron, kanban) | data still used? needs a check | 417 MB, 1,082 .py | migrate whatever hermes-rs still reads; archive the rest | M | own |
| /opt/galion/galion-light (incl. shadow/hermes-agent) | Python | shadow copies and the Hermes home | not running | 4.9 GB, 8,641 .py | delete after archiving | L | 3rd/own |
| /opt/galion/crew-skills/scripts | Python ×~20 + sh ×7 | skill sync and lint tooling | manual use only | 24 MB, 745 .py | one Rust `skills` CLI | M | own/3rd |
| /opt/galion/from-grok-build, galion-src, arena | Python | build imports, sources, arena agents | not running | 661/366/96 MB; 1,029/1,010/200 .py | review each one, then archive or port | M | mixed |
| /opt/galion/venv, tooling/node | Python/Node runtimes | interpreters | used by console (node) | 185 MB + — | goes away once console-static and the Hermes removal are done | — | 3rd |
| /opt/galion/stt, tts, vision | models + 11 .py (vision) | voice and vision assets | not running on the VPS | 1.7/3.3/2.5 GB | voice-rust; vision needs checking | M–H | own |

Total .py on disk (including venvs and site-packages) is about 13.0 M lines. That is mostly third-party code.

## 5. GPU pod (from repo references, not inspected live)
References: docs/legacy-python.md, crates/galion-one/src/speech.rs, docs/portability-inventory.md, TEACHER/*.
| Item | Lang | What | Replacement | Diff |
|---|---|---|---|---|
| tts_server.py | Python | TTS HTTP server | **voice-rust** (candle or ort for inference, axum for HTTP) | H |
| worker.py | Python | GPU job worker | Rust worker (tokio) inside voice-rust | M |
| faster-whisper (CTranslate2) | Python/C++ | STT | whisper-rs (whisper.cpp bindings) or candle-whisper; whisper.cpp itself is C++ **[RULING]** | M–H |
| CUDA / driver stack | C/C++ | GPU runtime | none | **[RULING]** |

## 6. Top 10 by priority
1. **console.service Node/Vite dev server** → console-static. It is the biggest non-Rust process (1.26 GB) and runs a dev server in production.
2. **Legacy Hermes Python** (hermes.service, tooling/hermes-agent, galion-light, /usr/local/bin/hermes) → archive and remove. It is already replaced by hermes-rs, so this is a cheap win.
3. **galion-backup.sh** → Rust galion-backup. It is small, but the backup path is critical.
4. **GPU tts_server.py + worker.py** → voice-rust.
5. **faster-whisper** → whisper-rs/candle, as part of voice-rust.
6. **fail2ban** → guard-rust + nftables **[RULING]**.
7. **Console TSX/TS UI** (≈4k lines) → Rust/WASM UI, once console-static is serving it. Browser-side JS needs **[RULING]**.
8. **scripts/real + scripts sh** (≈4k lines, 34 files) → `xtask` Rust test and dev runner.
9. **deploy/, contrib vps/bin, edge install, guard sh** (≈1.1k lines) → `galion-deploy` Rust CLI; guard → guard-rust.
10. **crew-skills Python tooling** → Rust skills CLI. Then postfix/networkd-dispatcher removal and cloudflared **[RULING]**.

Housekeeping: stray `.new` binaries are running from /root/growth-20261018 (hermes-gateway --mock, openmind-memory ×2). They are Rust but sit outside the deploy tree.

## Unverified
- Toolchain of the galion-desk, galion-one, tuner and tanga-serve ELFs.
- Contents of galion-offsite-serve.
- Whether hermes-rs still reads /opt/galion/hermes.
- The GPU pod itself.
- Contents of the stt and tts directories.
