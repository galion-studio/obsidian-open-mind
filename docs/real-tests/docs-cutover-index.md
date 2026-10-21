<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-server record: docs-cutover-index (slot 184), 2026-10-21 CEST (+02:00)

Docs-only row (docs T2.5, D19): writes `docs/cutover/README.md`. No binary was built or started; nothing was written, started, stopped or POSTed on the VPS.
Method: two `ssh slavik-vps 'TZ=Europe/Zagreb nice -n 10 bash -s'` batches. No secrets directory, EnvironmentFile or `Environment=` value was read;
from config.yml only `hostname:`/`service:` lines for galion.app/www/code were printed.

| Batch | Time (+02:00) | What | Result |
|---|---|---|---|
| 1 | 11:36:35–11:36:37 | running services; per unit is-active/is-enabled/MainPID/NRestarts/ActiveEnterTimestamp/DropInPaths, `readlink` + sha256 of `/proc/<pid>/exe` (18 names); 15 loopback GETs; `ls -d` of 9 backup paths; sha256 of 4 old binaries + galion-deploy/queue/tuner; config.yml mode/sha/galion lines; hermes mask; nerve /state keys; working copy HEAD | 14 Galion units active (incl. galion-nerve-shadow, openmind-queue); drop-ins exactly as in README §1; GETs as README §3 (all 200/302 except 8002 503); 9/9 backup paths present; config 644, `e3825ce4…`, galion.app + www → 127.0.0.1:8091, 0 code.galion.app lines, 37 hostnames; hermes masked → /dev/null; nerve openmind-nerve live cycle 226 score 61 D; WC 2f5a730 |
| 2 | 11:38:46 | full sha256 of the 8 binaries put live today + config.yml; hermes unit backup sha; cloudflared backup sha + mtime; drop-in directory listings; growth archive paths; queue/shadow FragmentPath + UnitFileState | 9/9 full sha256 equal the values in README §2; hermes unit backup `50d882e2f52b59f5` (= CALL-RECORD 08:24:55); backup config `f1525033…` (= "before" in CALL-RECORD 08:33:23), mtime 2026-10-18 22:36:14; drop-in files = 50/60 names listed; openmind-queue enabled, galion-nerve-shadow disabled |

Document checks on the box (11:39–11:42):
- 14/14 relative links in README.md resolve in this tree.
- 13/13 40-hex commit ids resolve as commits (base.git or the named swarm worktrees).
- 19 sha256 16-hex prefixes: 18 found in batch-1 output; the 19th (`50d882e2f52b59f5`) found in batch 2.
- Rollback lines for C1–C7, C9, C10 copied from their records (deploy-2026-10-21.md §Rollback, hermes-legacy-retire.md:136, poseidon-deploy.md §Rollback, galion-one-deploy.md §Rollback, nerve-eventlog-nerveurl-20261021.md §Rollback / CALL-RECORD 09:31:06, 09:32:33). C8 composed here, not rehearsed (no VPS write allowed in this row).
- Real-time observation: the cortex has no unit; its output is the `cortex` keys inside the live nerve's /state (openmind-nerve aa77864), read at 11:36.
