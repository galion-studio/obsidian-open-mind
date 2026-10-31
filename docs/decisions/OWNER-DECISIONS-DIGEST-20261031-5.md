<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Owner decisions digest 5 (for Slavik and Mate)

Prepared 2026-10-31 ~10:00 CET (+01:00) by wave-4 slot 449. Docs only, no
builds. Follows digest 4 (`swarm/owner-decisions-digest-4` @ 5b7c8b96, 10-28
21:45 CET). Sources: `/workspace/swarm/CALL-RECORD.md` up to 10-29 08:45 CET,
the 10-31 triage report, and fresh read-only VPS checks on 10-31 09:45 CET
(running binary path and start time per unit; `git merge-base --is-ancestor`
on the box for which slot is inside which live build). Nothing was applied,
deployed or merged by this slot.

## 0. The short version
- **Most of what digest 4 called "next deploy windows" is now live.** Every
  live core service runs a build that already contains the wave-4 work:

| service | live binary | running since (CET) | contains |
|---|---|---|---|
| cortex | gaia-cortex-4eaedea-llm | 10-30 12:38 | 431 4b036bc9, 440 3a8f6ff1 (5026d9cf, 9ece3df4, 12fa83a1) |
| nerve | openmind-nerve-4323ebb | 10-30 13:55 | 430 4ebd4260, 435 d19f643e, 448 18a5a72b (674482f1, 50a3011c) |
| console (both units) | galion-console-serve-ba4c011c | 10-30 | 433 a96e4c6e, candidate-2 c2522a99 |
| hermes-rs | hermes-gateway-6da98fdc | 10-31 08:41 | 426 ccca5545 (scoped memory) |
| openmind-memory | openmind-memory-f5331831 | 10-29 21:44 | 426 ccca5545, 388 1d7164b4 |

- Nerve self-score is **91 A** (was 68 D in digest 4); edge 0 hosts failing.
- **Not live yet:** console survival fold bf5ffc7d (425), voice-mouth
  e0037b52 (the live nerve already says voice-worker "not deployed yet"),
  and the post-publish land batch onto main (447, rehearsed today).
- OpenMind tool server **openmind-mcp-stack-6652177f** installed 10-31 09:28
  CET with a root status timer (every 5 min); Hermes picks it up at its
  **next** restart (not restarted by us). Spend limit $10, stop $9.50.

## 1. Resolved since digest 4

| ID | Answer / outcome |
|---|---|
| D-420-1 | **yes** (10-29 06:32): components-live-d420 installed with nerve 430+435. |
| D-421-1 / D-415-1 | **B, replay** (10-29 06:32); replay branch 432 3cdd6a4c. |
| D-423-1 / D-423-2 | accept / keep refuse (10-29 06:32). |
| D-424-1 | **default build** (10-29 06:32), used for nerve f66974c3, now superseded by 4323ebb. |
| D-424-3 | yes (INTERIM console-static line). |
| D-412-1, D-417-1/2/4 | done: cortex 440 content live in 4eaedea. |
| D-413-1 | done: console candidate-2 content live in ba4c011c. |
| D-426-1..3, D-403-1 | scoped memory recall shipped in hermes 6da98fdc + memory f5331831 (memory went first, as D-426-2 asked). |
| D-373-3 / D-416-2 | gateway line replaced: live hermes is 6da98fdc, not the 989958be candidate. |
| D-424-2 / D-405-3 | memory >= 1d7164b4 live (f5331831). |
| D-431-2 | answered option C. |
| D-435-2 | fold stays **off**. |
| D-400-2 | **coded** in 444 (2cd16cc0, local): all 10 hermes-tools sandbox tests go through the VPS gate. |
| D-428-2 (tree half) | **coded** in 445 (acdeb884, local): dist/bin binaries git rm'd, reproducible recipe. |

## 2. Still open, one line each (recommendation in bold)

**From digest 4 (not re-checked on the VPS today unless stated)**
- **D-361-1** restore test swap to galion-backup-dd89bea4, keep .sh as .pre361: **yes** (~5 min, owner runs).
- **D-398-1** 17-row venv o+w plan: **yes** in a VPS window; **D-398-2** hidden node_modules: **yes, later**; **D-398-3** secrets dir: **owner looks**.
- **D-406-1..4** archive legacy root vectors.db / keep 30 d / drop root entry (427 a075c7a9 staged) / leave manifest: **yes, yes, own slot, yes**.
- **D-414-1** console./dashboard.galion.studio: **A retire** (433 removed the names in code, live since ba4c011c); **D-414-2** reword F-396-4: **yes**.
- **D-420-2** state.rs/replay.rs fix b3d71822: in the live nerve line via 50a3011c; **mark done** if you agree.
- **D-420-3** tuner quick tunnel: **no, leave unpublished**.
- **D-407-1** post-publish land order: **yes**; rehearsed in 447 (see §3). **D-407-4** `>= 1` assert: applied in 447.
- **D-407-2 / D-411-2** nerve line onto main: one dedicated slot, **re-scoped** to the live 4323ebb line (437 marked stale).

**From slots 425-448**
- **D-425-1** install console survival fold drop-in: **yes**, but rebuild on ba4c011c first (441 marked stale: it targeted c2522a99 / rollback 5aa31927).
- **D-427-1..3** backup legacy root: **yes / info / yes**.
- **D-428-1** history purge: **not while private**; **D-428-3** deny-list as CI secret: **yes**; **D-428-4** land publish-gates-main: **yes**.
- **D-430-3**: still open (see 430 doc).
- **D-433-1..4, D-434-1..3**: content live or recipe recorded; anything left is listed in the slot docs.
- **D-444-1** gate openmind-sandbox's own 31-test suite the same way: **yes, follow-up slot**.
- **D-445-1** land dist-bin-release-plan: **yes**; **D-445-2** publish binaries as release assets: **not now**.
- **Hermes:** live behaviour test (paid, owner said **skip**); keep or switch off `propose_self_change`: **owner**.

## 3. Next windows (one page)

| window | candidate | staged | rollback | note |
|---|---|---|---|---|
| Hermes restart (owner picks time) | yaml already points openmind at openmind-mcp-stack-6652177f | /etc/hermes-rs/mcp-servers.yaml | copy back `mcp-servers.yaml.bak-omcp-6652177f`; `systemctl disable --now openmind-mcp-status.timer` | fixes spend_status / backup_status for Hermes |
| Console survival | rebuild 425 bf5ffc7d onto ba4c011c (new slot) | one drop-in carrying --owner-routes + survival flags (F-425) | rm drop-in -> ba4c011c | 441 stale |
| Land onto main | 447 swarm/post-publish-land-batch-2 (local) | none (git only) | drop branch | then 432 replay, then nerve line |
| Mouth | e0037b52 not needed for score today | — | — | D-358-* stays owner's |

## 4. Not in this digest
Older items from digests 1-3 not named above still stand (edge D-380-*,
mouth D-358-*, Hermes off-host D-306, bank reader D-351-1/D-365-1).
