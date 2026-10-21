<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Tuner cutover plan (first single-service cutover)

- Status (refreshed 2026-10-21 17:30 CEST (+02:00), wave-4 slot 203): **EXECUTED 2026-10-21 04:32:32 CEST (+02:00)** with the Rust galion-deploy, following [../ops/cutover-tuner-nerve.md](../ops/cutover-tuner-nerve.md).
  `/opt/galion/tuner/bin/tuner` `ecad72a4…3c91` → `89c7fcbcb54c510887dafd8c67b255f05d00595af598fe65f022e204c0ef2b12` (source integration-4 42de672e1e4b39a2c44e290195feb01a17df992e). A mistaken rollback at 04:32:10 was redeployed at 04:32:32.
  Approved by Mate 04:22 (Slavik auto-accept 20 Oct). Record: `swarm/vps-deploy-20261021@d9bd8295935d2620587aaed68e4b1b52f6ea72b9:docs/ops/deploy-2026-10-21.md`; index [README.md](README.md) §2 C2.
  At 17:22 the unit was active since 04:32:32 with NRestarts 0, the exe sha256 matched, and `/health` returned 200. Rollback: `/opt/galion/rust/bin/galion-deploy tuner /var/backups/galion/deploy/tuner-20261021-043232+0200.bin http://127.0.0.1:8650/health`.
  The sections below are the 2026-10-20 plan, kept as written; `tuner.galion.app` still has no DNS record.
- Original status (2026-10-20): PREPARED, NOT EXECUTED. Needs Slavik's approval. Nothing has been copied to the VPS, and no unit was restarted or edited.
- Prepared: 2026-10-20 19:10 CEST (+02:00) by swarm slot 07, branch `swarm/tuner-cutover` (based on GitHub main 13a296ce).
- Evidence: `docs/real-tests/tuner-cutover.md`. Re-run with `crates/galion-realtest/tests/real_sh_tuner_cutover.rs`.
- If the cutover runs on or after 2026-10-25 03:00, timestamps are CET (+01:00).

## 1. What runs as the tuner today (read-only probes, 2026-10-20 18:51–19:08 CEST)

| Item | Live value |
|---|---|
| Unit | `/etc/systemd/system/tuner.service`, enabled, active since 2026-10-18 19:18:07 CEST (+02:00). No drop-ins. No restarts since then. |
| Unit text | `ExecStart=/opt/galion/tuner/bin/tuner`, `Environment=TUNER_ADDR=127.0.0.1:8650`, `Environment=TUNER_ROOT=/opt/galion/tuner/www`, `Restart=always`, `RestartSec=2`, `Nice=10`, `NoNewPrivileges=true`, `WantedBy=multi-user.target`. Runs as root (no `User=`). |
| Process | pid 1196273, memory 348 KB (peak 1.1 MB), 1 task |
| Binary | `/opt/galion/tuner/bin/tuner`, 508,680 B, mtime 2026-10-18 19:18 CEST, x86-64 static-pie, stripped, BuildID `30201c04…`, **sha256 `ecad72a4368a3c9155a676b772ffb7c0caf863884c8614d9fdfcb5604866dd25`** |
| Where it came from | Built by hand from `/opt/galion/tuner/src/main.rs` (2,900 B, sha256 `c5d8d85a…`, 2026-10-18 19:17 CEST). That is the "no matching build found" binary from LIVE.md. It is not a build of any repo commit, and `dist/bin/tuner` was never made. That is why `dist/bin/tuner` is missing. |
| Listener | `127.0.0.1:8650` only (`ss -ltnp`: `tuner`, pid 1196273, fd 3) |
| Static root | `/opt/galion/tuner/www/{index.html, app.js, pitch.js, style.css}`. All four are byte-identical (sha256) to `crates/tuner/www/` in main. The cutover replaces **only the binary**. The www root stays as it is. |
| Routes | `GET`/`HEAD` on `/`, `/index.html`, `/app.js`, `/pitch.js`, `/style.css` (from the root dir, `Cache-Control: public, max-age=300`); `/health` → `200 {"ok":true}\n`; every other path → `404 not found\n`; any method other than `GET`/`HEAD` → `405 method not allowed\n`; a missing whitelisted file → `500 missing file\n`. Query strings are ignored. Every response carries the same security headers and `Connection: close`. |
| Dependants | `tuner-quick.service` (a **transient** unit: `cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650`, a random trycloudflare URL; it will not survive a reboot). galion-health 8660 checks `tuner` (port 8650, http 200). galion-nerve 8670 lists `tuner` (DEMETER) and `tuner-quick` (AETHER) as healthy. There is no cloudflared named-tunnel ingress for 8650, and `tuner.galion.app` is NXDOMAIN. |
| Deploy tool | `/opt/galion/rust/bin/galion-deploy` (bash, 2,830 B). It has `[tuner]=http://127.0.0.1:8650/health` in its health table. `/var/backups/galion/deploy/` exists (it holds 2 galion-health backups). Disk: 75 GB free. `curl` is present. |

## 2. What will replace it

The `tuner` crate from main, with this branch's additions. Served behaviour is **byte-for-byte identical** to live on all 21 routes of `crates/tuner/tests/fixtures/requests.txt`, diffed against the live 8650 on 2026-10-20 at 19:06 and 19:07 CEST. HEAD and non-GET responses are derived from the live source and tested hermetically.

Differences from the live binary (none of them changes a served byte):
1. `tuner --version` prints `tuner 0.1.0` and exits 0 **without binding**. galion-deploy runs `"$NEW" --version` before it installs. The old binary ignores arguments and would start serving, and with 8650 free that would hang the deploy. Today it fails only because 8650 is busy.
2. `tuner --check` resolves the env, checks that the four whitelisted files are readable, and exits 0 or 1. It does not bind. `--help` prints usage. Any other argument exits 2, so a typo never binds a port.
3. A failed bind prints `tuner: bind <addr>: <error>` and exits 1. The old binary panics. Under `panic=abort` the same panic would be SIGABRT plus a possible core dump.
4. Optional `TUNER_CONFIG` file (`bind`/`root`). It is unset in the live unit, so nothing changes.
5. Each response goes out in one `write_all` instead of header then body. The bytes on the wire are the same.

### Artefacts (on the box only; not copied to the VPS)

`/workspace/swarm/artifacts/tuner/` (`SHA256SUMS` sits next to them):

| File | Bytes | sha256 | Notes |
|---|---|---|---|
| `tuner-x86_64-unknown-linux-musl` | 459,176 | `47a17df2234fdd5c741f13e9403228e1d4f4dc5f73ac7b41e7996d5e3c511370` | **the one to deploy** (VPS is x86_64). Static, no interpreter. RSS 304 KB on the box. |
| `tuner-aarch64-unknown-linux-musl` | 412,104 | `0aca8af615620766c8dbeb4eca9ad9439473d6c0049c88613193c6d59f180b75` | Static. 21/21 routes identical under qemu-aarch64. For Pi/phone nodes, not the VPS. |

Build: `nice -n 10 cargo build --release --locked -p tuner --target <triple>` with `CARGO_TARGET_<TRIPLE>_LINKER=rust-lld`, cargo 1.99.1, profile `opt-level=z, lto, codegen-units=1, panic=abort, strip`. The build is **reproducible**: a second build from a different source path and target dir gave the same two sha256s. SOURCE-OF-TRUTH rule 6 says deploys come from a merged main commit, so after Slavik merges this branch, rebuild from that main commit (`crates/galion-realtest/tests/real_sh_tuner_cutover.rs` does it) and deploy only if the sha256 is still `47a17df2…`. If the merge changed the tuner crate, the sha will differ. Use the new sha, and re-run the script first.

## 3. The cutover (2026-10-20 plan; executed 2026-10-21 04:32 via galion-deploy, see Status at the top)

### 3.1 Copy the binary (needs approval; done 2026-10-21 04:30 as /var/tmp/galion-cutover staging, see Status)

Use a scratch dir that is **not** the `/opt/galion/openmind` working copy:

```
ssh slavik-vps 'install -d -m 0700 /var/tmp/galion-cutover'
scp /workspace/swarm/artifacts/tuner/tuner-x86_64-unknown-linux-musl /workspace/swarm/artifacts/tuner/SHA256SUMS slavik-vps:/var/tmp/galion-cutover/
ssh slavik-vps 'cd /var/tmp/galion-cutover && sha256sum -c --ignore-missing SHA256SUMS && chmod 0755 tuner-x86_64-unknown-linux-musl'
```

### 3.2 Preflight on the VPS (read-only; does not bind)

```
ssh slavik-vps '
  sha256sum /opt/galion/tuner/bin/tuner        # must still be ecad72a4…dd25
  systemctl is-active tuner                    # active
  curl -s -m 3 http://127.0.0.1:8650/health     # {"ok":true}
  /var/tmp/galion-cutover/tuner-x86_64-unknown-linux-musl --version      # tuner 0.1.0
  TUNER_ROOT=/opt/galion/tuner/www /var/tmp/galion-cutover/tuner-x86_64-unknown-linux-musl --check   # 4x ok, exit 0
'
```

### 3.3 The cutover command (exactly this)

```
ssh slavik-vps '/opt/galion/rust/bin/galion-deploy tuner /var/tmp/galion-cutover/tuner-x86_64-unknown-linux-musl http://127.0.0.1:8650/health'
```

What it does (read from the script): it checks the unit and resolves `TARGET=/opt/galion/tuner/bin/tuner` from ExecStart, checks for an ELF, and runs `--version` (the new binary exits at once). It copies the old binary to `/var/backups/galion/deploy/tuner-<YYYYmmdd-HHMMSS+zzzz>.bin`, runs `galion-backup.sh` (data snapshot, about 3 s; `SKIP_DATA_BACKUP=1` skips it, and the tuner has no data), installs the new binary atomically (`install` to `.new`, then `mv`), and runs `systemctl restart tuner`. It then polls `/health` once a second for up to 20 s. On a 2xx it logs `deployed`, keeps the last 5 backups, and exits 0. Otherwise it puts the backup back, restarts, and exits 1 (`rolled back ok`), or dies with `ROLLBACK ALSO UNHEALTHY`.

The unit file is **not** changed: ExecStart, env, Restart and the `[Install]` link stay. Do **not** use `deploy/staged/tuner.service` for this cutover. It points ExecStart at `/opt/galion/openmind/dist/bin-ws/tuner`, inside the working copy that must not be touched.

### 3.4 Health checks after the switch (one read each, no retry loops)

```
ssh slavik-vps '
  systemctl is-active tuner; systemctl show tuner -p MainPID -p NRestarts
  readlink /proc/$(systemctl show -p MainPID --value tuner)/exe     # /opt/galion/tuner/bin/tuner
  sha256sum /opt/galion/tuner/bin/tuner                              # 47a17df2…1370
  curl -s -m 3 -i http://127.0.0.1:8650/health                       # 200, {"ok":true}
  curl -s -m 3 -o /dev/null -w "%{http_code} %{size_download}\n" http://127.0.0.1:8650/          # 200 2356
  curl -s -m 3 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8650/nope                      # 404
  journalctl -u tuner -n 3 --no-pager -o cat                          # "tuner 127.0.0.1:8650 serving /opt/galion/tuner/www"
  curl -s -m 3 http://127.0.0.1:8660/status.json | grep -o "\"tuner\"[^}]*}"   # ok:true
'
GALION_REAL=1 GALION_REAL_VPS=1 cargo test -p galion-realtest --test real_sh_tuner_cutover -- --nocapture # on the box: all 21 routes vs the now-new live 8650, expect 21/21 IDENTICAL
```

The fixtures in `crates/tuner/tests/fixtures/live/` are the old binary's bytes. After the switch, the full route diff against live is the strongest check. It is read-only GETs.

## 4. Rollback

- **Automatic:** galion-deploy restores the backup if `/health` is not 2xx within 20 s.
- **Manual (new binary healthy but wrong):**
  `ssh slavik-vps '/opt/galion/rust/bin/galion-deploy tuner /var/backups/galion/deploy/tuner-<STAMP>.bin http://127.0.0.1:8650/health'`.
  `<STAMP>` is printed in the `backup=` field of the deploy log line. This also backs up the new binary, then restores the old one through the same health-checked path. Check: `sha256sum /opt/galion/tuner/bin/tuner` = `ecad72a4…dd25`.
- **Last resort (deploy tool broken):** `install -m 0755 /var/backups/galion/deploy/tuner-<STAMP>.bin /opt/galion/tuner/bin/tuner.new && mv -f /opt/galion/tuner/bin/tuner.new /opt/galion/tuner/bin/tuner && systemctl restart tuner && curl -s http://127.0.0.1:8650/health`.
- The old binary also stays rebuildable from `/opt/galion/tuner/src/main.rs`, and the www root is never touched.

## 5. Expected downtime

- **Planned: well under 1 s of refused connections on 127.0.0.1:8650**, the `systemctl restart` gap (SIGTERM to the old process, which exits at once with no handler; exec of the new static binary; bind). On the box a stop-then-start of the musl binary left the port closed for 3–4 ms (5 runs). systemd overhead adds tens of ms. The whole `galion-deploy` run takes about 5 s (about 3 s data backup, then the first health poll after 1 s), and the old binary serves for most of that.
- Visible effect: during the gap, `tuner-quick`'s cloudflared gets connection-refused for any request (it retries per request; the tunnel itself stays up, and its trycloudflare URL does not change). There were 0 established connections on 8650 at 19:08 CEST.
- **Worst case (new binary unhealthy):** up to about 20 s of failed health before the automatic rollback, plus about 1 s for the restore restart.

## 6. Go / no-go checklist for Slavik

Go only when every box is ticked:

- [ ] **Approval:** Slavik approves *this one* service (tuner, 8650), and records it in `docs/cutover-approval.md` (today it says `tuner | no`).
- [ ] **Merged:** `swarm/tuner-cutover` is merged to main through a PR with the self-eval gate green (fmt, clippy `-D warnings`, tests, no-python).
- Note 2026-10-21: on integration-2 the strict galion-common parser answers the 2 malformed rows `bare-get` / `no-version` with 400, so expect 19 IDENTICAL + 2 KNOWN-DIFF until the owner decides; see docs/real-tests/realscript-port-3.md.
- [ ] **Rebuilt from main:** `crates/galion-realtest/tests/real_sh_tuner_cutover.rs` on the merged main commit reports 21/21 IDENTICAL against live 8650 and 3/3 real tests passed, and the x86_64 sha256 is recorded (expected `47a17df2…1370` if the crate is unchanged by the merge).
- [ ] **Copied and verified:** the binary is in `/var/tmp/galion-cutover/` on the VPS and `sha256sum -c` is OK (step 3.1, needs approval).
- [ ] **Preflight green (3.2):** the live binary sha is still `ecad72a4…dd25`, `tuner` is active, `/health` is 200, `--version` prints `tuner 0.1.0`, and `--check` exits 0.
- [ ] **Window:** not during `galion-backup.timer` (runs about every 6 h; the next run was 20:21 CEST on 2026-10-20; check `systemctl list-timers galion-backup.timer`).
- [ ] **Nobody depends on 8650 at that moment** beyond tuner-quick (the 1 s blip is acceptable).
- [ ] **One service only:** no other cutover in the same session (docs/next.md: one at a time).
- [ ] **Rollback understood:** the backup path is printed in the deploy log, and the manual rollback command (section 4) is at hand.

**No-go if:** the live sha256 changed (someone redeployed by hand), the merged-main build differs from the tested binary and was not re-run, `--check` fails on the VPS, galion-deploy's text differs from the one read on 2026-10-20 (2,830 B), or the backup timer is due within 5 minutes.

## 7. Open points (not blocking this cutover, separate approvals)

- **tuner-quick (SEC-5 / O4):** a transient trycloudflare tunnel exposing the tuner publicly at a random URL, which does not survive a reboot. Keep, replace with `deploy/staged/tuner-publish.service` (the `tuner-publish` binary is on ws/one-desk-tuner, not in main), or stop. Not part of this cutover.
- The live tuner runs as root with only `NoNewPrivileges`. A later unit hardening (`DynamicUser=yes`, `ProtectSystem=strict`, `ReadOnlyPaths=/opt/galion/tuner/www`) would be a unit change with its own approval.
- One thread per connection with a 5 s read timeout, the same as live. It is fine behind loopback and a tunnel; a connection cap would be a behaviour change and is left out on purpose.
- `deploy/staged/tuner.service` still names a working-copy path. Fix it, or delete it in a follow-up.

## 8. Box rehearsal (2026-10-20, swarm/deploy-sim)

Every step of sections 3 and 4 was rehearsed on the box with the real old and new binaries, a systemd-shaped supervisor and galion-deploy's real code: `docs/cutover/tuner-rehearsal.md` (21/21 checks, downtime 2–3 ms per swap or rollback). Four rules come out of it and apply on top of this plan: (1) use the L2 manual rollback only while the unit is active, and if it is down use the last resort, because the live galion-deploy waits forever for `--version` and the old tuner ignores it and serves; (2) add `systemctl reset-failed tuner` before every manual restart; (3) after any non-zero galion-deploy exit, check `is-active` and the sha, because exit 1 is ambiguous; (4) run galion-deploy under `nohup` with a log file.
