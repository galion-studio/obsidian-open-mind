<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: cutover-prep (swarm slot 38)

- Branch `swarm/cutover-prep` on `swarm/deploy-rust` 787a54d. Part 1 (the galion-deploy port) is deploy-rust's work. This branch adds only `galion-deploy backup` / `backup-verify`, the runbook `docs/ops/cutover-tuner-nerve.md`, and the read-only findings below.
- Times are Europe/Zagreb, CEST (+02:00), 2026-10-20.
- **Nothing was deployed, installed, restarted or POSTed on the VPS.** The probes were reads (`systemctl show/cat/status`, `ss -ltn`, `stat`, `sha256sum`, `ls`, `grep -l`/`grep -o` for setting names, and HTTP GET). One disclosure: the auto-accept probe wrote the temporary file `/tmp/.aa-opt.txt` on the VPS and deleted it in the same command.

## 1. Pre-cutover backup: `tests/real_backup.rs`

```
cargo build --release -p galion-deploy --examples --bins
GALION_REAL=1 cargo test --release -p galion-deploy --test real_backup -- --nocapture
```

Run 2026-10-20 23:30:00 +02:00 on the release binary, which ran as a child process, and re-run at 23:44:40 +02:00 (release galion-deploy sha256 prefix d2bac478be80f88c). It passed both times, in 0.14 s and 0.16 s. The tar sha changes per run because the fake root's mtimes do; the 23:44:40 run gave `2d3daaa1…d7f2`. The fake root has the VPS shape: the **real live bytes** of galion-nerve (`3b150dea…6d3d`) and of the bash galion-deploy (`86207af5…aea0b`), www files of live size, a 13,089 B state.json, plus a canary file under `/opt/galion/galion-light/secrets`, one under `/root/galion-secrets`, and `/var/lib/galion-nerve/stray.env`.

| check | result |
|---|---|
| snapshot dir | `<dest>/20261020-233000+0200/`, 0700. backup.tar, MANIFEST.tsv, SHA256SUMS, VERIFY.txt are 0600 |
| overlay | 23 members, tar 1,315,840 B, sha256 `e888c2a4…ab93` (23:30 run). GNU `tar -tf`/`-xpf` reads it, and an extracted member is byte-equal |
| data part | `galion-backup run` stand-in (`examples/fake_galion_backup.rs`) got `GALION_BACKUP_DEST=<snap>/data`, `KEEP=1`. Its archive `galion-20261020-231900+0200.tar.gz` is in SHA256SUMS and the manifest header |
| secrets | neither canary appears in the tar, the manifest, or the log. stray.env is skipped by name only (`skipped … secret-like file name (not read)`) |
| verify | clean → OK. A live file changed → OK + `drift` note. A tar byte flipped → FAIL. The data archive changed → FAIL |
| refusals | unknown unit → exit 1. A required path missing → exit 1 and **no dir created** |
| real galion-backup (rust-ports 26a2a8e, gnu build 1,470,432 B) | exit 1 on the box (`tar: no files to archive`: no production paths, root is fixed at `/`). galion-deploy reports `overlay written and verified …, but galion-backup run exited 1`, exit 1, and the overlay still verifies |

Manifest of that run (fake root; on the VPS the sizes and shas will be those of section 3):

```
f 0755 450568 067c56be… /opt/galion/tuner/bin/tuner            (box stand-in, not the live tuner)
d/f       /opt/galion/tuner/{src/main.rs,test/test.js,www/{app.js,index.html,pitch.js,style.css}}
f 0644    76 fd316ebf… /etc/systemd/system/tuner.service
f 0644    98 361e5d62… /run/systemd/transient/tuner-quick.service
f 0755 816456 3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d /opt/galion/rust/bin/galion-nerve
f 0644   111 eb420b64… /etc/systemd/system/galion-nerve.service
d/f       /var/lib/galion-nerve/{.probe,state.json (13089),self-eval/latest.json,self-eval/latest.md}
f 0755  2830 86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b /opt/galion/rust/bin/galion-deploy
# note: absent (optional) /etc/systemd/system/tuner.service.d, galion-nerve.service.d (none at baseline)
# note: skipped /var/lib/galion-nerve/stray.env: secret-like file name (not read)
```

Gates at 23:43–23:45 +02:00: `cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` 130 suites with 546 passed and 0 failed (GALION_REAL unset), `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` scanned 197 and found 0 missing.

Also on this tree: `cargo test -p galion-deploy` gave 33 unit tests (28 of deploy-rust + 5 new: ustar round trip with long names, corrupt header, secret filter, manifest row parse, plan), plus systemd_backend 1/1, real_build 1/1, and **deploy-rust's real_rehearsal 4/4 (204 s, GALION_REAL=1, debug)**, unchanged by the backup addition. Those rehearsals use deploy-rust's ports 19390-19399, not slot 38's. Afterwards no listeners were left.

## 2. Read-only VPS inventory (2026-10-20 23:13:33–23:13:34 +02:00, `/state` at 23:22:55 +02:00)

See the table in `docs/ops/cutover-tuner-nerve.md` §0. Raw output (no secret values; env values were not printed) is on the box at `/workspace/swarm/sim/cutover-prep/vps/inventory.txt`.

## 3. Auto-accept search (2026-10-20 23:13:58–23:14:21 +02:00; file paths and setting names only, no values read or printed)

| where | path(s) | setting name(s) |
|---|---|---|
| systemd units (`/etc/systemd/system`, `/run/systemd/transient`, `/lib/systemd/system`) | no match in an active unit. The disabled old `hermes.service` ExecStart has a flag | `--accept-hooks` |
| live Hermes config | `/root/.hermes/config.yaml` (same content as `/opt/galion/hermes/config.yaml`), plus `config.yaml.pod-model`, `config.yaml.bak-20261018-193614+0200`, `backups/config/config.yaml.good.*` (5) | `hooks_auto_accept`, `galion-auto-accept` |
| | `/root/.hermes/guard.json` | `autoAccept` |
| | `/root/.hermes/control.json` | `auto_accept` |
| galion-light Hermes home | `/opt/galion/galion-light/hermes-home/config.yaml`, `backups/config/config.yaml.good.20260922-150330` | `hooks_auto_accept` |
| console UI/API that toggles it | `/opt/galion/console-ide/src/routes/api/hermes/guard.ts` | `autoAccept`, `galion-auto-accept` |
| | `console-ide/src/routes/api/v1/security.ts`, `src/routes/_console/security.tsx`, `src/components/hermes-panel.tsx` (copies in `.vercel` output, `/opt/galion/from-grok-build/workspace`, `galion-light/docs/build-session/grok-mirror`) | `autoAccept` |
| hermes-agent code | `/opt/galion/tooling/hermes-agent`, `galion-light/shadow/hermes-agent` | `hooks_auto_accept`, `SIMPLEX_AUTO_ACCEPT`, `DIALOG_POLICY_AUTO_ACCEPT`, `auto_accept` (config_defaults) |
| env files | `/etc/*.env`, `/opt/galion/hermes/.env`: names only via `grep -l`, **no match** | — |
| noise | 2,983 hits under /opt/galion: 2,006 in `hermes/sessions` + 795 in `hermes/archive` (conversation logs), 20 node_modules, venv site-packages (openai, playwright), llama.cpp (false positives), `docs/build-session/09-auto-accept.md` | — |

The secrets dirs were excluded from the search.

## 4. Slips (disclosed)

- While writing an rg/grep pattern on the box, I typed `python3 --version`. The `/workspace/swarm/bin` guard blocked it (`BLOCKED: Python is forbidden`), so nothing ran.
- The `/tmp/.aa-opt.txt` temporary file on the VPS (above).
- My first take duplicated the galion-deploy port (local commit 4b6face, never pushed). Per steering it was dropped, and the branch was restarted on deploy-rust 787a54d. Only the backup code was carried over.
