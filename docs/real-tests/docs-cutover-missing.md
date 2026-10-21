<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-server record: docs-cutover-missing (slot 203), 2026-10-21 CEST (+02:00)

Docs-only row (follow-up to slot 184 D-I1, D-I4). It writes 8 unit docs in `docs/cutover/`, refreshes the status lines of `nerve.md`, `tuner.md`, `tuner-rehearsal.md` and
`../ops/cutover-tuner-nerve.md`, and updates the index `docs/cutover/README.md` (C11, C12). No binary was built or started; ports 20730-20739 unused.
Nothing was written, started, stopped or POSTed on the VPS.

Method: `ssh slavik-vps 'TZ=Europe/Zagreb nice -n 10 bash -s'` batches, plus two single read-only `grep` calls.
No secrets directory, `EnvironmentFile` or `Environment=` value was read. Unit and drop-in files were printed with every line matching
`environment|token|key|secret|pass` filtered out, and only the count of `Environment=` lines was kept. From config.yml only the `hostname:`/`path:`/`service:` lines for
one.galion.app, /tanga, 8091 and forge were printed. Host name and address lines are kept out of this file.

| Batch | Time (+02:00) | What | Result |
|---|---|---|---|
| 1 | 17:22:05–17:22:06 | for memory, hermes-rs, galion-one, openmind-queue, galion-health, tanga-proto, link-watch-rs, galion-backup .service/.timer, galion-nerve, tuner: `systemctl show` (state, MainPID, NRestarts, since, FragmentPath, DropInPaths, User, limits), unit + drop-in sha256/size/mtime and filtered text, `/proc/<pid>/exe` path/sha256/size/RSS; `ss -ltn`; 14 loopback GETs; galion-health names; nerve `/state` keys; backup paths; old binaries; `/var/backups/galion` listing; backup journal; timer; WC HEAD | all 9 long-running units active; exe sha256 = values in the docs (9/9); drop-ins exactly as documented (memory 2, hermes-rs 2, galion-one 1, nerve 3, others 0, galion-backup offsite.conf); GETs: 8420 200, 8642 200, 8091 /health 200 and / 200, **8002 503**, 19120 /healthz 200 and /stats 200, 8660 /healthz, /status.json (ok:true, 12 names) and / 200, 8671 /tanga/ 200, 8670 /health 200, /events 200, 8650 200; nerve openmind-nerve cycle 28 **mode dry-run** 71/C; 11/11 backup paths present; 4 old binaries present with the recorded sha256; 12 archives, runs 02:21, 08:18, 14:17 ok; next timer 20:21:46; WC 2f5a730 |
| 2 | 17:23:35 | ZFS check of `size_kb`; `/root/galion-switch-20261018-2140/` names, rollback-script usage lines (filtered); tanga backup names; galion-one backup listing; `.d` backup contents; link.json mtime; journal counts; tanga restart history; tanga HEAD headers; legacy unit states; failed units; queue `/stats` counters | fs zfs; newest archive `stat` 18,612,783 B, `du -k` 18,191 now vs logged `size_kb=1` (B2 confirmed); rollback.sh = [all\|memory\|hermes\|link-watch] → Python/Node; rollback-gaps.sh = [all\|memory\|link-watch]; link.json mtime 17:23:34; tanga 3 restarts all 2026-10-18 22:34:25–35; noindex + CSP present; link-watch, memory-gateway disabled/inactive, hermes masked; failed = 3 old run-u1109x; queue ready 0 leased 0 replayed 3 quota_refusals 3 budget_refusals 1 |
| 2b | 17:24 | `grep` of rollback.sh / rollback-gaps.sh (filtered); `grep approv\|owner\|slavik` of the SWITCH-LOG files | targets as in batch 2; no approval line in the 2026-10-18 switch logs (one unrelated "approval" mention) |
| 3 | 17:28:33–17:28:34 | cloudflared config sha/mode/size/mtime + matching rule lines + hostname count; cloudflared since; both cloudflared backups (size/mode, sha of 150844); tuner backups; nerve 170637 `taken-at`; nerve `mode` file present?; nerve /state; 9 GETs; MainPID/NRestarts of 9 units; WC HEAD | config `b421acb1…1432` 3,420 B 644, mtime 17:08:54; `/tanga` rule (lines 14–16) before `one.galion.app` → 8091 (18–19); `forge.galion.studio` → 127.0.0.1:18800 (130–131); 38 hostnames; cloudflared since 17:08:54 NRestarts 0; backup 150844 sha256 `e3825ce4…d976` (3,257 B, **mode 644**; 063215 is 600); `tuner-20261021-043232+0200.bin` present (508,680 B); nerve backup taken 17:06:37; mode file absent; nerve cycle 41 dry-run 61/D; GETs as batch 1 (8002 503); MainPID and NRestarts **unchanged** vs 17:22 on all 9 units; WC 2f5a730 |

Box-side source checks (17:24–17:26):
- `/workspace/galion-one-design/tanga-serve/target/x86_64-unknown-linux-musl/release/tanga-serve` sha256 `9c540eeb…a1de7f7` = live tanga-serve.
- `git show vps/ws-switch-gaps:deploy/link-watch-rs.service | sha256sum` = live unit `1abe890e…bb23`; `dist/bin/link-watch` on vps/ws-switch-gaps and e43d7ab = `6b9d04de…` ≠ live `2f5874f7…`.
- `e43d7ab:deploy/foundation/galion-backup.sh` sha256 = live `0897efc0…80a4` (as docs/galion-backup.md).
- Records read: nerve-eventlog-deploy 0ddce9b, galion-one-deploy a93218d, poseidon-deploy d2c5cf1, nerve-dryrun-live 80974b9 (local worktree), CALL-RECORD up to 17:19.

Real-time observation: the cortex has no unit of its own. Its output is the `cortex` keys in the live nerve `/state` (openmind-nerve aa77864), read in batches 1 and 3.
Between them the nerve score moved 71/C → 61/D, and `galion-one-mouth` 503 is the standing degrading component.

Document checks on the box: see the final report of slot 203 in CALL-RECORD (links, commit ids, sha prefixes, SPDX, no-python, repo-check).
