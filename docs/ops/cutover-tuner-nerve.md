<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Runbook: tuner + galion-nerve cutover with the Rust galion-deploy

Branch `swarm/cutover-prep` (slot 38), on top of `swarm/deploy-rust` 787a54d.

**Status (refreshed 2026-10-21 17:30 CEST (+02:00), wave-4 slot 203):** this runbook was **used for the tuner** on 2026-10-21. The §2 backup ran at 04:31:16,
galion-deploy went bash → Rust at 04:31:47 and the tuner was deployed at 04:32:32 (a mistaken rollback at 04:32:10 was redeployed). Approved by Mate 04:22 (Slavik auto-accept 20 Oct).
Record: `swarm/vps-deploy-20261021@d9bd8295935d2620587aaed68e4b1b52f6ea72b9:docs/ops/deploy-2026-10-21.md` ([../cutover/README.md](../cutover/README.md) §2 C1/C2).
The **nerve was not cut over with this runbook.** It went live at 09:43:54 by an ExecStart drop-in `50-rust-bin.conf` (openmind-nerve aa77864, old binary kept), and reflexes were set to dry-run at 17:08:27 (`70-nerve-dryrun.conf`).
See [../cutover/nerve.md](../cutover/nerve.md) Status. The text below is the 2026-10-20 plan, kept as written.
Original line (2026-10-20): Nothing in this file has been run on the VPS.

Every step that changes the VPS
needs Slavik's approval for that step (`docs/cutover-approval.md`). Times are
Europe/Zagreb. The per-service detail stays in `docs/cutover/tuner.md` and
`docs/cutover/nerve.md`. This runbook is the order of operations for doing both
with the Rust tool, plus the pre-cutover backup.

## 0. Live state (read-only probe 2026-10-20 23:13:33–23:14:21 +02:00, plus GET /state at 23:22:55 +02:00)

| | tuner | galion-nerve |
|---|---|---|
| unit file | `/etc/systemd/system/tuner.service`, 324 B, sha256 `8d47534506f8…` | `/etc/systemd/system/galion-nerve.service`, 1,737 B, sha256 `12084a2891f5…` |
| drop-ins | none (`DropInPaths=` empty, no `tuner.service.d`) | none |
| Restart / start limit | `always`, 2 s / 5 starts per 10 s | `always`, 10 s / 5 starts per 10 min |
| main PID, since, NRestarts | 1196273, Sun 2026-10-18 19:18:07 +02:00, 0 | 1466797, Sun 2026-10-18 22:35:04 +02:00, 0 |
| binary (= running exe) | `/opt/galion/tuner/bin/tuner` 508,680 B `ecad72a4368a3c9155a676b772ffb7c0caf863884c8614d9fdfcb5604866dd25` | `/opt/galion/rust/bin/galion-nerve` 816,456 B `3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d` |
| env names (values not read) | TUNER_ADDR, TUNER_ROOT | NERVE_ADDR, NERVE_INTERVAL, NERVE_DATA_DIR, NERVE_DRY_RUN, NERVE_FAIL_CYCLES, NERVE_REPORT_EVERY |
| data | `/opt/galion/tuner/{src,www,test}`, 528,208 B in total including the binary | `/var/lib/galion-nerve` (0700), 544,923 B: `state.json` 13,089 B, `.probe`, `self-eval/` (about 95 hourly JSONs, `latest.json`, `latest.md`). The `mode` file is **absent** |
| health (GET) | `:8650/health` 200 (12 B), `/` 200 (2,356 B) | `:8670/healthz` 200, `/` 200 (3,900 B), `/state` 200 (13,089 B), **`/health` 404** |
| dependants | transient `tuner-quick.service` (cloudflared quick tunnel, 291 B in `/run/systemd/transient`, gone after a reboot) | galion-health `:8660/status.json` 200 |

- **Nerve reflexes are live.** `/state` says `"mode":"live"`, with rate limits `reflex_restarts_per_hour` and `rollback_per_unit_per_day` 1. Its rollback reflex runs `/opt/galion/rust/bin/galion-deploy` through `systemd-run`.
- The deploy tool on the VPS is still the **bash** script: 2,830 B, `86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b`, with F1/F2/F3 and no galion-nerve health entry.
- `/opt/galion/rust/bin/galion-backup`: 1,599,472 B, mtime 2026-10-18 22:13. Its behaviour against swarm/rust-ports 26a2a8e is not verified (see 2.2). `galion-backup.timer` last ran 2026-10-20 20:22:05 +02:00; next run 2026-10-21 02:20:50 +02:00.
- `/opt/galion/backups/` exists and holds `tuner-20261018-191738+0200/`. `/var/backups/galion/deploy/` holds 2 galion-health `.bin`. 80.47 GB free.

## 1. Owner decisions before anything runs

1. **D1: Rust galion-deploy on the VPS?** (docs/real-tests/deploy-rust.md §7.1.) This runbook assumes **yes**: install it before the cutovers. If the answer is no, use docs/cutover/{tuner,nerve}.md as written for bash, and do 2 with the box-built `galion-deploy backup` copied to `/var/tmp/galion-cutover/` (it does not depend on the installed tool).
2. **D2: nerve reflexes to dry-run first** (nerve.md §3.0). With `mode` live, the old nerve can run galion-deploy *during* our window: a rollback or a restart of tuner. The new nerve sees more inputs and may pick reflexes the old one never picked. Recommended: `echo dry-run > /var/lib/galion-nerve/mode` before step 3, and keep it for 24 h after the nerve swap.
3. **D3: secrets in the data archive.** `galion-backup run` (one definition of the backup) archives env and admin files, as the bash backup always did. The snapshot dir is 0700/0600 under root. Decide whether the pre-cutover copy may hold them, or run with `--no-data` and rely on the 02:20 timer archive.
4. **D4: window.** Not 2026-10-25 01:00–04:00 (DST change), and not within 5 min of the backup timer (02:20).

## 2. Pre-cutover backup (`galion-deploy backup`)

### 2.1 What it captures

**One definition of a backup lives in galion-backup** (swarm/rust-ports, `galion-backup run`, byte-parity with the bash backup). `galion-deploy backup` does not reimplement it. It runs the galion-backup binary with `GALION_BACKUP_DEST=<snapshot>/data GALION_BACKUP_KEEP=1`, and adds only what a binary swap can break:

| captured by galion-deploy (overlay `backup.tar`) | why |
|---|---|
| `/opt/galion/tuner/bin/tuner` | the tuner rollback target (also copied by galion-deploy itself to `/var/backups/galion/deploy/`, kept to 5 there) |
| `/opt/galion/tuner/{src,www,test}` | hand-built source of the live binary and the static root it serves |
| `/etc/systemd/system/tuner.service` (+ `tuner.service.d/` if it appears) | unit text and drop-ins |
| `/run/systemd/transient/tuner-quick.service` (optional) | transient: it is the only record of the quick tunnel's definition |
| `/opt/galion/rust/bin/galion-nerve` | the nerve rollback target |
| `/etc/systemd/system/galion-nerve.service` (+ `.d/`) | unit text and drop-ins |
| `/var/lib/galion-nerve/` (state.json, mode if present, self-eval/) | reflex state, rate-limit history, mode switch |
| `/opt/galion/rust/bin/galion-deploy` | the deploy tool that is about to be replaced (bash today) |

Not read, ever: `/opt/galion/galion-light/secrets`, `/root/galion-secrets`, and any path with a component that contains `secret` or `credential`, or a name that is `.env`, `*.env`, `*.key`, `*.pem`, or contains `token`, `id_ed25519` or `id_rsa`. Skips are listed as notes in the manifest, by name only.

Layout: `/opt/galion/backups/<YYYYMMDD-HHMMSS+zzzz>/` (Zagreb, 0700), holding `backup.tar` (ustar), `MANIFEST.tsv` (type, mode, uid, gid, size, Zagreb mtime, sha256, path), `SHA256SUMS`, `VERIFY.txt` and `data/` (galion-backup's archive + `galion-backup.log`). Every file is 0600. The tool verifies after writing: it re-reads the tar, compares each member with the manifest, and checks `SHA256SUMS`, including the data archive. Exit codes: 0 written and verified; 1 failed (a missing required path refuses before any dir is created; a galion-backup failure leaves a verified overlay and says so); 2 usage.

### 2.2 Commands (needs approval: writes under /opt/galion/backups, reads root-only files)

```
# box: the binary from this branch, and the galion-backup from swarm/rust-ports 26a2a8e
#   (/opt/galion/rust/bin/galion-backup on the VPS predates 26a2a8e; do not assume it has `run`)
scp galion-deploy galion-backup SHA256SUMS slavik-vps:/var/tmp/galion-cutover/
ssh slavik-vps 'cd /var/tmp/galion-cutover && sha256sum -c --ignore-missing SHA256SUMS'

ssh slavik-vps '/var/tmp/galion-cutover/galion-deploy backup --galion-backup /var/tmp/galion-cutover/galion-backup'
#   or, under D3 = no secrets:  ... galion-deploy backup --no-data
ssh slavik-vps '/var/tmp/galion-cutover/galion-deploy backup-verify /opt/galion/backups/<STAMP> --live-root /'
```

Expect about 23 overlay members, about 1.9 MB of tar (tuner 528 KB + nerve 816 KB + nerve state 545 KB + units), and `drift: none` right after. Restore one path: `tar -xpf /opt/galion/backups/<STAMP>/backup.tar -C / opt/galion/rust/bin/galion-nerve`.

## 3. Cutover order

All commands go under `nohup` with a log file. The Rust tool also catches SIGHUP (F6), but the log is the record.

1. **Install the Rust galion-deploy (D1).** Copy and `sha256sum -c`, then `install -m 0755 galion-deploy /opt/galion/rust/bin/galion-deploy.new && mv -f …new /opt/galion/rust/bin/galion-deploy`. The bash copy is in the snapshot (2.1). Check `galion-deploy --version` → `galion-deploy 0.1.0`. No unit restarts; nerve's `deploy_in_progress()` still matches `galion-deploy ` in a cmdline.
2. **Nerve mode → dry-run (D2):** `cat /var/lib/galion-nerve/mode 2>/dev/null; echo dry-run > /var/lib/galion-nerve/mode`. The file is absent today, so the rollback for this step is `rm`.
3. **Tuner:**
   `nohup /opt/galion/rust/bin/galion-deploy tuner /var/tmp/galion-cutover/tuner-x86_64-unknown-linux-musl http://127.0.0.1:8650/health > /var/tmp/galion-cutover/deploy-tuner.log 2>&1; echo exit=$?`
   No manual `reset-failed` needed (the tool does it, F2). Expected sha after: `47a17df2…1370` (or the merged-main rebuild).
4. **Nerve:**
   `nohup /opt/galion/rust/bin/galion-deploy galion-nerve /var/tmp/galion-cutover/openmind-nerve-x86_64-unknown-linux-musl > /var/tmp/galion-cutover/deploy-nerve.log 2>&1; echo exit=$?`
   The Rust table has `galion-nerve → :8670/healthz`, and the rollback is judged on `/healthz` (F4). **Never pass `/health` for nerve.** Expected sha after: `ad9a5b29…106a`.

Between 3 and 4: wait for galion-health `:8660/status.json` to show tuner up, and look at nerve's `/state` (dry-run decisions only).

## 4. Health checks (GET only)

| | after tuner | after nerve |
|---|---|---|
| endpoints | `:8650/health` 200 `{"ok":true}`; `/` 200 2,356 B; `/nope` 404 | `:8670/healthz` 200; `/` 200; `/state` 200 with `"mode":"dry-run"` (D2) |
| unit | `systemctl is-active tuner`; `sha256sum /opt/galion/tuner/bin/tuner` | `systemctl is-active galion-nerve`; sha of `/opt/galion/rust/bin/galion-nerve` |
| side | tuner-quick still active (its URL does not change) | `:8660/status.json` 200 |

## 5. Exit codes and what to do (Rust tool, F3)

| exit | meaning | action |
|---|---|---|
| 0 | deployed, healthy | step 4 checks |
| 1 | refused, nothing changed (bad args, not ELF, lock held by another deploy, e.g. a nerve reflex) | read the log, fix, rerun |
| 2 | usage | — |
| 3 | health failed, **rolled back and healthy** | old sha is back; stop, investigate the new build |
| 4 | **rollback unhealthy**: manual action | `systemctl reset-failed <u> && systemctl restart <u>` (38 ms in the rehearsal); if still bad, restore the binary from the snapshot (2.2) or `/var/backups/galion/deploy/<u>-<STAMP>.bin` |
| 5 | interrupted, old binary restored, healthy | rerun when ready |
| 6 | interrupted, restore unhealthy | as for 4 |
| killed (137) | journal `<backup-dir>/<u>.journal` remains | `galion-deploy --recover <u>` (exit 3 = old binary back) |

Manual L2 rollback after a successful deploy: `galion-deploy <u> /var/backups/galion/deploy/<u>-<STAMP>.bin` (the stamp is in the `backup=` log field). With the Rust tool this is safe even when the unit is down: `--version` is bounded at 5 s (F1).

## 6. Expected timings (box rehearsals, swarm/deploy-rust 787a54d, 2026-10-20 23:20:48 +02:00; tuner 24/0, nerve 19/0, room 7/0, desk 8/0)

- Planned swap: **2 ms** refused connections for tuner, **2–4 ms** for nerve. On the VPS, add systemd overhead (tens of ms) and about 3 s of data backup inside the deploy (the old binary serves during it).
- L1 automatic rollback: **about 20.2 s** of failed health (the 20 × 1 s window, kept on purpose), then exit 3.
- F1 (the unit is down while an old binary is tried): up to 5 s more, then its process group is killed.
- F2 with the default reset-failed: exit 3 and active, even with nerve's 5-per-10-min window full.

## 7. Go / no-go

Go only if all of these hold: D1–D4 are decided; the snapshot of 2.2 exists and `backup-verify` is OK; the live shas and the unit-file shas still match section 0 (the binaries, `8d47534506f8…`, `12084a2891f5…`); `/healthz` and `/health` are 200 before the start; the merged-main builds match the rehearsed shas or were re-rehearsed; and no galion-deploy process is running (`pgrep -af 'galion-deploy '`; a nerve reflex may be in flight).

## 8. What makes a live cutover unsafe today

- Nerve in **live** reflex mode with a deploy-capable rollback reflex (D2).
- The bash galion-deploy is still installed: the F1 hang on an L2 rollback with the unit down, the F2 start-limit trap, and an ambiguous exit 1. The bash table has no nerve entry, so it judges nerve on `is-active` only, and on `/health` 404 if that URL is passed.
- The VPS galion-backup binary is unverified against rust-ports 26a2a8e (2.2).
- tuner-quick is transient: a reboot during the window loses the public quick URL for good.
- The whole flow has been proven only against simsup and systemctl shims. PID 1 on the box is tini, not systemd.
