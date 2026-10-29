# Hermes teaching inventory — 2026-10-29 (t373u, read-only)

Compiled 2026-10-29 ~12:00 CET (Europe/Zagreb, +01:00). Nothing was changed, restarted or deleted. No git commands were run in /opt/galion/openmind. The VPS IP and all secrets are redacted (`<IP>`); paths under `~/skills/imported-from-vps/<VPS-IP>/` are written as `<VPS-IP>`.
VPS `ls` times are UTC; they are converted to CET below.

## 0. Sources read
- Voice-call transcripts of this bot (Samantha · Galion One voice): `/home/box/agent-data/agents/07d10d52-…/voice-calls/*.json` (16 calls, Oct 18, Oct 20 and Oct 27).
- ReadTranscript for New Bot (b719722c…) and New Bot 1 (63f6437e…): about 40% sampled and grepped. These cover KANADER, the autobody/frame work and swarm slots, with no Hermes teaching content. Main-chat text sessions of this bot can't be read with ReadTranscript from a subagent (only my own run and the other agents).
- RecallMemory (bot and shared memory).
- Box: /workspace/swarm (root docs, reports, CALL-RECORD.md, LIVE-FIX-WATCH.md, wt/*), /home/box/skills, /home/box/workflows. /cursor/stores/* holds only an empty `user` mount and this bot's store, with nothing relevant. /workspace/cargen was not touched.
- VPS (ssh slavik-vps, read-only cat/ls/grep/tar -t).

## 1. What the owner said about teaching Hermes and what it should memorize (dated)

| When (Zagreb) | Source | Owner's words (Mate relaying Slavik) |
|---|---|---|
| Sun 2026-10-18 19:22 CEST | `/opt/galion/openmind/admin.preferences` rule 2 (also `docs/session-2026-10-18-evening.md` l.58) | "Take the terminal, learn the KMS (key management system) and the research agent, sort all of that out, copy all the files (whatever the existing setup says), copy it and teach/train it." Rule 3: "No instructions or manuals for Slavik… Everything must be done automatically by the agents on the VPS." |
| Sun 2026-10-18 19:25 CEST | `/opt/galion/openmind/docs/main.md` §14 | "**Always learn files. Files that we are building.**" Agents must read main.md, goals.md, admin.preferences, persona files, session docs and any new file before acting, and treat every change as new knowledge. |
| Sun 2026-10-18 19:53 CEST | voice call 71c2d4bb | "…spawn multiple groups, like … 50 agents at once, and let's build this thing. … **let's Hermes agent be the boss. Teach him, parent him, show him how to do it, and don't forget about the data.**" Grok Bot then became the teacher and Hermes the boss (session `openmind-boss-20261018`); see the session doc line "Grok Bot is teacher to the Hermes swarm". |
| Wed 2026-10-21 | memory / survival SKILL | PERMANENT survival rule: no wasteful spend, Flash-class models under $5, nothing paid on timers. Source: Hermes skill `skills/galion/survival/SKILL.md`. |
| Wed 2026-10-21 22:50 CEST | `skills/galion/cortex-escalation/SKILL.md` (Mate t217) | Hermes as boss: "**You change nothing.** No code, config, unit, drop-in, service start/stop/restart … until Mate replies with an explicit approval of this exact proposal." |
| Tue 2026-10-27 18:21–18:25 CET | voice call e91f3edf | Persona switch to Samantha, plus "save everything on the virtual private server". At 18:25 the assistant reported: "Hermes now has the full call context — I wrote it into its docs, its memory, and both survival skill files." Owner at 18:25: "**Everything needs to be proven. … please don't lie to me. … every test that we do, it needs to be based on the real life circumstances.**" |
| Tue 2026-10-27 ~19:22 CET | t301u ("Mate t301u part 2") | The teaching pass itself. Results are in `/workspace/swarm/reports/t301u-hermes-teaching.md`. **Gap:** Mate's own t301u request text is not in any file or transcript I could read (it was in the main chat). |
| Thu 2026-10-29 (t377u, relayed by parent) | steering | Everything is sandbox-tested before live, and Hermes may not make live changes. |

Owner intent in summary: Hermes is to be **taught and parented** by Grok Bot as the swarm "boss". It should **learn the files being built** (main.md, goals.md, admin.preferences, personas, session docs) before acting, and **memorize** these things:
- Standing rules: survival/cash, no Python, Zagreb time, no secrets.
- The call context: off-VPS survival, the Teglenice/government infrastructure gap, the Samantha persona.
- The t301u problem-solving standards: diagnose, verify, use numbers, escalate, and apply the overload gate.
- Real-life proof and no invented output.

## 2. Where the files live

### VPS
- `/opt/galion/hermes/` (= `/root/.hermes` → `/workspace/hermes` → `/opt/galion/hermes`, all one directory):
  - `docs/TEACHING-PROBLEM-SOLVING.md` (2026-10-27 19:22 CET; md5 is identical to the box copy)
  - `docs/PRACTICE-t301u.md` (19:22 CET)
  - `docs/CALL-CONTEXT-20261027.md` (19:22 CET; includes a Teaching (t301u) section)
  - `memories/MEMORY.md` (19:23 CET), `memories/USER.md`, plus backups `MEMORY.md.bak-t273done` and `MEMORY.md.bak-t285u-20261027172432`
  - `SOUL.md` (Oct 18, +CJK hygiene section; earlier copy `SOUL.md.bak-20261018-164336`), `USER.md`, `control.json`, `guard.json`, `channels/galion-one.md`, `config.yaml` (legacy Python-Hermes config; `memory:` provider memory_tencentdb, nudge 10; `skills.external_dirs` /opt/skills/{local,addy,ondrej/skills}; `agent.system_prompt` 4-sentence rule)
  - `skills/galion/survival/SKILL.md` (+ `.bak-20261027-181851` = old pod version, `.bak-t285u` = interim Rust-path version)
  - `skills/galion/cortex-escalation/SKILL.md`, plus about 35 other galion skills (self-eval, deploy-lane, global-agent-guardrails, grade, workflows, one-voice …) and stock skill categories
  - `backups/config/config.yaml.good.*` (Oct 17–18) and `.curator_backups/blobs/*`
- `/opt/galion/openmind/TEACHER/`: LESSON.md, BOSS.md (+ `.bak-20261018-223613+0200`), QUEUE.md, CHECKS.md, TICKETS.md, PROGRESS.md, BOSS-PROGRESS.md, BACKLOG.md, STAGED.md (+ .bak), A-foundation.md
- `/opt/galion/openmind/release/TEACHER/`: LESSON.md, CHECKS.md, TICKETS.md, PROGRESS.md, AWAITING-APPROVAL.md
- `/opt/galion/openmind/admin.preferences`, `docs/main.md`, `docs/goals.md`, `docs/session-2026-10-18*.md`, `personas/persona-plex-v1.{md,toml}`, `stack/hermes/*` (redacted Hermes config/SOUL copy)
- `/opt/galion/galion-light/state/hermes-briefs/CALL-CONTEXT-20261027.md` (18:24 CET; the pre-teaching version, without the t301u section)
- `/opt/galion/galion-light/state/boss/`: boss.db (Oct 18), contest.json/contest-run/ (Oct 5 worker contest), skill-loop/ (catalog.json, decisions.jsonl, loop.log, Oct 5)
- `/etc/systemd/system/hermes-rs.service` + `.d/{50-rust-bin,60-nerve-url,70-research-budget,90-deploy-t291u}.conf`

### Box
- `/workspace/swarm/reports/t301u-hermes-teaching.md`, `t301u-hermes-teaching-lesson.md`, `t301u-hermes-reply.content.txt`, `t301u-hermes-reply.redacted.txt`, `t301u-teacher-facts.json`
- `/workspace/swarm/hermes-call-context-20261027.md`, `HERMES-RESEARCH.md`, `CALL-RECORD.md` (owner-request log), `LIVE-FIX-WATCH.md`
- `/workspace/swarm/reports/t287u-{A,B,C}-sandbox-*.md`, `overnight-test-hermes.md`, `overnight-morning-report-20261028.md`, `slavik-status-20261028.md`
- Repo copies of TEACHER/ and stack/hermes/ in many worktrees, e.g. `/workspace/swarm/wt/hermes-model-picker/TEACHER/*` (newest, 2026-10-29 11:00) and `wt/*/stack/hermes/{SOUL.md,control.json,channels/…}`
- `/home/box/workflows/vps/`: main.md, goals.md, admin.preferences, session-2026-10-18{,-evening}.md, personas/persona-plex-v1.md, study/A-foundation.md, study/B-nervous-system.md, study/bin/hermes-q.mjs (read-only state.db query script), hermes-openrouter-proposal.md, plus .bak copies
- `/home/box/skills/imported-from-vps/<VPS-IP>/persona/opt/galion/hermes/{SOUL.md,USER.md,archive,channels}`, `…/galion-light/{SOUL.md,PERSONALITY_EXTRACT.md,NIGHT_INTERN.md,hermes-home/SOUL.md}`, `…/arena/homes/g2-{0..3}/{SOUL,TASK}.md`, `…/opt/galion/galion-light/skills/*/SKILL.md`
- Voice transcripts: `/home/box/agent-data/agents/07d10d52-6941-4f04-bb94-d496e248ebd3/voice-calls/*.json`

### Backups
- `/opt/galion/backups/grokbot-box-20261027-1823-cet/` + `.tar.gz` (138 entries; 18:26 CET): docs/hermes-call-context-20261027.md, docs/HERMES-RESEARCH.md, hermes/survival-rust-plan.md, hermes/found-docs.txt, survival logs, worktree patches. It does **not** contain the t301u teaching files, which were written after it (19:22).
- `/var/backups/grok-bot-box-20261027-172514+0100/` (meta, workspace, ssh, agent-data, home-box). Taken 17:25 CET, before teaching.
- `/var/backups/galion/galion-YYYYMMDD-*.tar.gz` (6-hourly) + `offsite/*.age`: these contain the dbs (hermes-rs-state.db, hermes-legacy-state.db, memory vectors), `opt/galion/hermes/config.yaml`, `.env`, `var/lib/memory-tencentdb/persona.md`. They contain **no** `hermes/docs`, `memories/MEMORY.md`, SOUL.md, skills or TEACHER/.
- `/var/backups/hermes-legacy-20261021/` (Python Hermes archive: shadow-/tooling-hermes-agent.tar hold stock SOUL.md, memory docs, plugin-catalog/hud-teach.yaml)
- `/var/backups/growth-20261018/` + `growth-20261018-archive-20261021.tar.gz` (shadow-{old,new}/persona.md, scene-openmind-boss-20261018.md, bench files)
- `/var/backups/hermes-rs.service-*` (unit snapshots), `/root/galion-switch-20261018-2140/` (rollback for the Rust switch)

## 3. What the teaching materials contain

- **TEACHING-PROBLEM-SOLVING.md** (t301u lesson): five non-negotiable standards.
  1. Diagnose before acting: nproc, /proc/loadavg, free -h, vmstat, process and thread counts.
  2. Verify your own results.
  3. Report facts with numbers.
  4. Escalate when a gate fails.
  5. Overload gate for the 8-core VPS: green ≤~8, amber 8–12, red ≥~12 → SPAWN_BLOCKED.
  It also gives the answer format (ending with SPAWN_OK or SPAWN_BLOCKED and the reason), a practice prompt, and operator limits: no deploy/merge/restart, respect the USD caps, never print keys.
- **PRACTICE-t301u.md**: a read-only practice prompt on high loadavg vs. low %CPU.
- **t301u-hermes-teaching.md** (box, the teacher's report): a practice call to `POST 127.0.0.1:8642/v1/chat/completions` (model hermes-agent, HTTP 200, 4141 tokens). Hermes said it had no terminal (only clock and delegate_task) and got the gate right (SPAWN_BLOCKED) for the wrong reason.
  - Grades: diagnose = partial, numbers = fail, gate = pass/partial.
  - Teacher's measured load: 19.66/21.68/20.67 on 8 cores.
  - A CORRECTION was appended to MEMORY.
- **MEMORY.md** (Hermes durable memory), in order:
  - Older facts: laptop model galion RTX 4060 is weights only; host ip50 has 8 cores and no GPU; call the terminal tool and never invent output; no shell on the laptop; France spare until 15 Oct; OpenRouter is not this turn's model; browser off; guard on.
  - The 2026-10-27 call context (t283–t285).
  - Off-VPS survival thread (snapshot + heartbeat + resume, tip aa77864, PR only).
  - The Samantha switch.
  - The t273 deploy facts.
  - The t301u standards.
  - The t301u CORRECTION: terminal diagnostics are not spawning; cite the stated load; mark hypotheses as unmeasured.
- **memories/USER.md**: raise Hermes's capability; Hermes Research is for whitehat study; auto-accept; speak as a colleague, result first; a session code word.
- **CALL-CONTEXT-20261027.md**: off-VPS survival goal (the Teglenice/government infrastructure gap of about 1.5 years), related tasks t273/t274/t276/t282, PR #13, the Samantha persona source of truth, the VPS snapshot, operator stance, and the t301u teaching pointer.
- **survival/SKILL.md**: read survival.json first; refresh with the Rust galion-survival (no Python); prefer Flash under $5; Grok parked until a real receipt; no new pods; don't restart the gateway, keep.sh, Whisper or vision; "done" means a file, score or health code changed; guardian-tick; Off-host continuity section (2026-10-27): PR-only until an authorized deploy.
- **SOUL.md**: Hermes is "the operator of Galion Studio".
  - Standing goal: stay alive on this server; laptop model through :11434; France spare.
  - Voice rules: short, plain, verify before calling something done.
  - Steam/Diesel admin read, Guard/denylist, hands on loopback :8642, Obsidian vault memory format (frontmatter tags), Zagreb clock rules, the Hearth file.
  - Replies are 4 sentences in 2 paragraphs; CJK hygiene.
  - Parts are stale: it says "laptop galion is the model", but the live hermes-rs routes are `hermes-agent`/`galion` → OpenRouter claude-haiku.
- **TEACHER/LESSON.md** (2026-10-18 18:58 CEST): the OpenMind foundation lesson. Ten HARD RULES: no Python, no Revolut/money, Zagreb RFC 3339 time, no CJK, CPU-light (max 3 subagents, nice), production untouchable, spare test ports, no secrets, no Hermes config changes, toolchain. Also how to work as LEAD (delegate_task ≤3, the worker doesn't grade itself, the boss runs CHECKS, PROGRESS lines, teacher corrections override) and the definition of done.
- **TEACHER/BOSS.md** (2026-10-18 20:00 CEST): "You are Hermes, boss of the Obsidian Open Mind swarm" on opus-5.5. Teacher messages override the plan. Always learn files first. The boss loop: workers don't commit; production changes go to STAGED.md "awaiting Slavik's approval", never applied. $10 stop. The box repo copy (10.8 KB) adds a 2026-10-20 SOURCE-OF-TRUTH reconciliation: GitHub is the source of truth, branch + PR, build on box/CI, max 3 agents in total. **The VPS copy is 7.85 KB and may lack that reconciliation (not verified line by line).**
- **TEACHER/QUEUE.md** (B01–B50 tickets in waves), **CHECKS.md** (C1–C11 + rubric), **TICKETS.md**, **PROGRESS.md** (T1–T6 PASS, 2026-10-18), **BOSS-PROGRESS.md** (graded runs through 2026-10-20, e.g. "PASS 9/10 … live pid not replaced"), **BACKLOG.md**, **STAGED.md** (production items awaiting approval), **A-foundation.md** (architecture study). **release/TEACHER/** is an earlier release-kit lesson with AWAITING-APPROVAL.md.
- **admin.preferences / main.md**: Slavik's rules: automatic, teach/train, no manuals, proactive contact, plus the plan sections. §8: Revolut is NOT approved. §14: always learn files.
- **cortex-escalation/SKILL.md** (Oct 21, t217): Hermes as boss diagnoses and proposes but changes nothing without Mate's explicit approval. DeepSeek must not write fixes.
- **channels/galion-one.md**: Samantha is warm, a few words then one short line; Jarvis is drier.
- **state/boss/**: an Oct 5 "contest" where boss hermes-local ran 3 workers (flint/keel/marsh, 5/5). The reward was "next local job goes to this worker first". skill-loop holds a catalog/decisions log.

### Sandbox / gating of Hermes's changes (for t377u)
- These already say Hermes must not make live changes, and stage instead:
  - cortex-escalation SKILL: change nothing until Mate approves.
  - BOSS.md/LESSON.md: production untouchable; production changes are only staged in STAGED.md; never systemctl enable/start/restart.
  - TEACHING-PROBLEM-SOLVING: no deploys/restarts; overload gate before spawning.
  - survival SKILL: PR-only until an authorized deploy.
  - Owner 2026-10-27: everything proven under real-life conditions.
- These conflict with or loosen t377u (flag for an update):
  - SOUL.md: "Every session … may use the terminal and execute_code without asking", "Do not ask permission to run a tool".
  - USER.md: "Auto accept: on".
  - control.json/guard.json: guard on, auto_accept true.
  - deploy-lane SKILL (legacy): it tells Hermes to copy files onto the live console tree, build, and restart vite preview on :8080.
  - Legacy skills (self-eval, grade, workflows) still call `python3 /workspace/...` paths, against the no-Python rule.
- Nothing in the teaching set yet says "sandbox-test first". The sandbox rule appears only in engineering docs (t287u reports, docs/real-tests/*) and the hermes-rs coding gate.

## 4. Hermes on :8642 and the Landlock history
- 2026-10-18 21:42 CEST: the dev VPS switched to Rust. `hermes-rs.service` on loopback :8642 replaced Python hermes.service. Its hardening: ProtectSystem=strict, ReadWritePaths=/var/lib/hermes-rs, ReadOnlyPaths=/opt/galion/hermes, MemoryMax=64M, NoNewPrivileges. Rollback is in /root/galion-switch-20261018-2140/.
- 2026-10-21: drop-ins 50 (binary aa77864), 60 (NERVE_URL) and 70 (research budget, session $5 / day $7 / 2000 calls). A sandbox test on the VPS found `landlock_abi None`. Memory note: "VPS kernel lacks Landlock … AppArmor active".
- 2026-10-27 19:08 CET (t291u): cutover of :8642 to `hermes-gateway-b7fbbc78` (coding + hermes-tools + openmind-sandbox compiled in) via `90-deploy-t291u.conf`. The drop-in says: "**Coding RUNTIME OFF** … Landlock absent on this PVE host — do not enable coding without ALLOW_NO_LANDLOCK + dedicated worktree (owner call)."
- 2026-10-27 ~19:22 CET: t301u teaching call over :8642 (HTTP 200).
- 2026-10-27 19:45:27 CET: an AUTO-ACCEPT attempt to turn coding on.
  - Worktree `/var/lib/hermes-rs/coding-worktree` hit DENY_ROOTS → exit 2 crash loop (13 times in 46 s); cortex opened its circuit breaker.
  - A /srv retry failed with EROFS (ProtectSystem=strict). The drop-in was removed; healthy again at 19:46:18.
  - Coding stays OFF (reports/overnight-test-hermes.md, LIVE-FIX-WATCH.md, slot 355).
- 2026-10-27 22:55 CET (slot 320): VPS retest as uid 65534. Plain run 2/8 (preflight refuses, no Landlock, fails closed); with ALLOW_NO_LANDLOCK=1 10/0. This gave owner decision **D-320-1** (options a/b/c).
- 2026-10-28: fixes staged only, none installed:
  - slot 334 `coding-tools-landlock-stage` f68def1d (pushed 16:05 CET): `HERMES_RS_CODING_NO_LANDLOCK=refuse|allow|layered`; staged drop-ins `landlock-alternatives/landlock-{a-allow,b-refuse,c-layered}.conf`.
  - slot 346 `hermes-sandboxed-require` 4828997d.
  - slot 355/381 crashloop fix: check-config validates coding env, `ON_ERROR=disable`, `RestartPreventExitStatus=2`, /srv/hermes-coding.
  - slot 397 `hermes-coding-dir-stage` 0e41453a (tmpfiles.d).
  - slavik-status-20261028: "coding tools OFF (VPS lacks Landlock; D-320-1 options staged)".
- **Whether the fix was applied: no, per docs and files.** Read today (2026-10-29 ~11:55 CET):
  - The drop-in dir still has only 50/60/70/90; no landlock-alternatives or coding drop-in is installed.
  - `/sys/kernel/security/lsm` = `lockdown,capability,yama,apparmor` (no landlock) on kernel 6.8.12-43-pve. A real fix needs the Proxmox host kernel/LSM boot parameter, which is outside this CT.
  - No later CALL-RECORD/LIVE-FIX-WATCH entry reports D-320-1 chosen or a coding drop-in deployed.
  - The live service state itself is covered by the separate check.

## Gaps
- Mate's own t301u request text (and other main-chat owner messages before Oct 27) is not readable from here. Only the voice transcripts and the files' summaries of them are available.
- The 6-hourly `/var/backups/galion` backups do not include Hermes docs, memories, SOUL or skills, or openmind TEACHER/. The grokbot-box 18:23 snapshot predates the t301u files. **The t301u lesson, practice and MEMORY correction exist only live on the VPS, plus box copies of the lesson and report.**
- `hermes-briefs/CALL-CONTEXT-20261027.md` is the older version, without the t301u section.
- The VPS TEACHER/BOSS.md (7.85 KB) differs from the repo copy (10.8 KB, with the SoT reconciliation).
- Hermes's API session had no terminal tool (clock + delegate_task only), so the practice couldn't be done as designed. No follow-up teaching or practice run with a tool-enabled path was found.
- The "Hermes as boss" session `openmind-boss-20261018` progress stops at 2026-10-20 (BOSS-PROGRESS.md). No later boss-teaching curriculum was found.
- No "curriculum" or "lessons" folder exists beyond TEACHER/ and docs/TEACHING-PROBLEM-SOLVING.md. `/opt/skills/{local,addy,ondrej}` (config external_dirs) were not inspected.
- SOUL.md/control.json still describe the laptop model as primary and auto-accept, which is stale against hermes-rs and t377u.
