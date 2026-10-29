<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# OpenMind fixes, 2026-10-29

Written 2026-10-29 at about 14:20 +01:00 (Europe/Zagreb, CET). The VPS clock is UTC, and every
time below has been converted to CET.

The approval came in chat at 13:45–13:46 +01:00. The steering message t392u came at
13:57 +01:00 (use River for test chats, keep calls modest, include a demo how-to).

## Blocker first: CI does not run

- Every GitHub Actions job in `galion-studio/obsidian-open-mind` since about
  2026-10-28 20:22 +01:00 has ended as `failure` in about 1 s.
  - No runner is assigned, there are 0 steps, and the job logs return 404.
  - The last green runs were on 2026-10-28, 18:12–20:22 +01:00.
  - It was still the same at 14:12 +01:00 today, on run 40879783737 (queued) and the runs before it.
- The rule is to merge only when CI and the sandbox pass, so **no PR was merged**: #15, #16 and #17
  are open.
- The cause is on GitHub's side and not proven. A billing or Actions-minutes limit is the
  likely suspect. Mate should check Settings → Billing → Actions for the org.
- Live deploys were made from the sandbox-proven branch builds. Binaries are named by commit
  sha, with a backup and a rollback for each step. main was not touched.

## Per task

### A. Rust openmind MCP server: done; live; PR open
- Branch `swarm/openmind-mcp-rust` at 498244ec. PR: https://github.com/galion-studio/obsidian-open-mind/pull/15
- What it has:
  - 8 tools: memory_search, memory_conversations, memory_recall, memory_save, vault_list,
    vault_read, vault_search, openmind_status.
  - The vault tools are read-only and refuse `..` paths.
  - memory_search falls back to conversation search when there are no structured (L1)
    memories.
- Local checks:
  - fmt and clippy `-D warnings` are clean.
  - Tests: 11 stdio + 2 hermes round-trip pass.
  - repo-check PASS.
  - The musl binary is 584,696 bytes.
- Sandbox, 14:05 +01:00:
  - Hermes `--mock` with MCP on logged "mcp: 8 tools".
  - list, read and search worked on copied notes, and `..` was refused.
  - memory save, search and recall round trip OK.
  - The vault tree hash was unchanged.

### B. Memory integration: done; live; PR open
- Branch `swarm/memory-integration-20261029` at 5c7bad5f. PR: https://github.com/galion-studio/obsidian-open-mind/pull/16
- Local tests: workspace 2278 passed, 0 failed, 18 ignored. The feature-pin test needed a
  `cargo fetch` first. clippy is clean, and the GALION_REAL=1 real tests are OK.
- Sandbox, 14:05 +01:00: memory and hermes ran with capture_ok 10, recall_ok 10.
- Backup: `/var/backups/omfix-20261029-20261029130707/` (14:07 +01:00). It holds the drop-ins,
  config.yaml, a vectors.db snapshot, a memory-tencentdb tgz and a hermes-teaching tgz.
- Live, 14:07 +01:00:
  - openmind-memory: `openmind-memory-5c7bad5f` via drop-in `91-omfix-5c7bad5f.conf`.
  - hermes-rs: `hermes-gateway-5c7bad5f` with `HERMES_RS_MEMORY=1` via drop-in
    `96-omfix-memory.conf`.
  - Both active, NRestarts=0. The log says "memory on … key_set=true scope=session health 200".
- Live check with River (`river-flash`, 2 turns, session omfix-live-1029a, 14:08 +01:00):
  - capture_ok 2, capture_err 0.
  - The store's newest write moved from **2026-10-19** (stale) to **2026-10-29 14:08:46 +01:00**.
    `openmind-memory freshness` says `state=fresh`, exit 0.
  - `/search/conversations` finds the saved codeword.
- Open finding: Hermes's automatic recall shows `recall_hit 0`.
  - `/recall` searches only L1 structured memories, persona and scenes.
  - L1 has not grown since 2026-10-18 (216 rows, newest 2026-10-18 21:41 +02:00), because
    no extraction pipeline runs.
  - Cross-session recall works through the MCP `memory_search` fallback (see D).
  - Follow-up: switch on L1 extraction, or add an L0 fallback to `/recall`.
- Follow-up (CI size gate): the musl `hermes-gateway-5c7bad5f` is 2,105,536 bytes. That is
  **8,384 bytes over** the 2 MiB size budget, so the ucpu size stage will fail once CI runs.
  Shrink it, or agree on a budget change.

### C. Memory freshness sensor: done; live
- Binary: `/opt/galion/rust/bin/openmind-memory-freshness-5c7bad5f`, a copy of the memory
  binary, with a .sha256 file.
- It was tested under `ProtectSystem=strict` with only `/var/lib/galion-nerve` writable:
  `state=fresh`, exit 0. No extra paths were needed.
- Nerve drop-in `/etc/systemd/system/galion-nerve.service.d/93-omfix-memfresh.conf`
  (14:09 +01:00):
  - NERVE_MEMFRESH_BIN, DATA_DIR=/var/lib/memory-tencentdb, INSTANCE=galion, MAX_AGE_H=24,
    EVERY_S=900, TIMEOUT_MS=10000.
- Result:
  - galion-nerve is active, NRestarts=0.
  - The `memory-freshness` sensor runs with findings 0 (fresh).
  - Nerve counts went from healthy 9 / degrading 2 / failing-soon 2 to healthy 11 /
    degrading 1 / failing-soon 1.
- Stale alarm test, in the sandbox only (14:05 +01:00): nerve f66974c with MAX_AGE_H=0 raised
  the `memory-stale` warn incident. At 24 h there was none.

### D. Hermes MCP live, vault read-only: done; live
- config.yaml:
  - The old Python `galion-stack` entry (`stack-read.py`) was replaced by `openmind` →
    `/opt/galion/rust/bin/openmind-mcp-stack-498244ec`, with `lazy: false`.
  - It was written with `cat` into place, so the mode is kept.
  - The diff touched only that block. Backup: `config.yaml` in the backup folder.
- Drop-in `/etc/systemd/system/hermes-rs.service.d/97-omfix-mcp.conf`: `HERMES_RS_MCP=1` and
  `ReadOnlyPaths=/opt/galion/galion-light/docs/build-session`.
- `95-cpu-always.conf` is unchanged. InaccessiblePaths is still
  `-/opt/galion/openmind -/opt/galion/rust/src`.
- Log at 14:10:22 +01:00: "mcp: 8 tools".
- Live check with River (new session omfix-live-1029b, 14:11 +01:00):
  - Hermes found the codeword from the other session through memory_search.
  - It listed the vault (51 notes) and read `00-summary.md`.
  - The vault tree hash was the same before and after.
- **SEC-3, report only (nothing changed):**
  - Vault `/opt/galion/galion-light/docs/build-session`: 26 of 332 files and 49 directories are
    world-writable (`find -perm -o+w`). The top folder is 775.
  - `/opt/galion/hermes/config.yaml` and `/opt/galion/hermes/SOUL.md` are 666 (world-writable).
  - There are 5 world-writable entries at the top level of `/opt/galion/hermes`. `.env` is 600.
  - Proposed fix, waiting for Mate's OK: `chmod o-w` on those paths.

### E. Vault repo checks (galion-studio/obsidian-open-mind)
- Visibility: **private**. Not changed.
- SECURITY.md: the fallback contact is still the `__CONTACT_EMAIL__` placeholder.
- Security contact:
  - **No official contact found.**
  - `security@galion.app` appears only in old Project Nexus docs in
    `galion-studio/galion-platform`. It is unconfirmed.
  - `security@nexuslang.dev` belongs to another product.
  - Nothing was invented.
  - TODO branch: `swarm/security-contact-todo-20261029` (c1ba435f), which adds
    `docs/todo/security-contact.md`. **Mate must give the address.**
- Open PRs:
  - Before this job (7): #14 hermes-offhost, #7 brand docs, and dependabot #6, #5, #4, #2, #1.
  - Added by this job: #15, #16, #17.
  - None can merge: CI does not run.
- Self-check: local repo-check PASS on every branch I pushed. The CI self-eval does not run (blocked).
- Overlap: another job pushed similar branches between 14:02 and 14:05 +01:00. I left them
  alone. They may duplicate this work:
  - `swarm/openmind-mcp-vault`
  - `swarm/openmind-write-guard-onmain`
  - `swarm/memory-freshness-sensor-onmain`
  - `swarm/hermes-memory-session-scope-onmain`
  - `swarm/hermes-rs-memory-client-onmain`
  - `swarm/memory-db-stale-diag-onmain`
- The gate `/opt/galion/gate/bin/hermes-gate` appeared at 14:05–14:08 +01:00, from another
  job, after my sandbox runs. I did not use it. My sandbox was the equivalent transient systemd
  unit: user nobody, PrivateNetwork, seccomp @system-service, NoNewPrivileges,
  ProtectSystem=strict, /opt/galion and /root inaccessible, and a private work folder. No
  Landlock.

### F. Teaching: done; live
- Backup branch `swarm/hermes-teaching-backup-20261029` (da03a976), under
  `backups/hermes-teaching/`:
  - It holds SOUL.md, USER.md, control.json and skills/.
  - Public IPs were changed to `<IP>` and emails to `<email>`.
  - Lock files were left out.
  - The 5 `.py` files were left out because the repo allows no Python. Their sha256 values are
    listed. The full copy stays only in the VPS backup.
- Sandbox-first branch `swarm/hermes-teaching-sandbox-first` (9dad0a7a). PR: https://github.com/galion-studio/obsidian-open-mind/pull/17
  - SOUL `## Vault`: the vault is read-only, read with the vault_* tools, and memory goes
    through memory_save/search.
  - New `## Changes`: "Sandbox gate first. Hermes never changes live code…".
  - cortex-escalation step 4 now requires the sandbox gate before a deploy.
- Python skills: none ported. The port list is in the PR README.
  - Feasible later: arxiv, grounded-citations, blocked-page-recovery, and the github credential
    helper (`gh auth git-credential`).
  - Not feasible now: vision `look.py`, loop, mesh, grade, trace, talk, and the TV bridge.
  - Note: hermes-rs loads only SOUL.md, not skills/.
- Mock load check in the sandbox (14:16 +01:00): health 200. Prompt tokens were 1686 with the
  new SOUL and 1534 with the old one, so the new SOUL is loaded.
- Live, 14:17 +01:00:
  - Backup: `…/teaching-pre-131725/`.
  - SOUL.md and the cortex-escalation SKILL.md were updated in place.
  - `systemctl restart hermes-rs`: active, NRestarts=0, /health 200, mcp 8 tools, memory on.

## Rollback (newest first)
1. Teaching:
   - `cat …/teaching-pre-131725/SOUL.md > /opt/galion/hermes/SOUL.md`
   - Do the same for `cortex-escalation.SKILL.md` →
     `/opt/galion/hermes/skills/galion/cortex-escalation/SKILL.md`
   - `systemctl restart hermes-rs`
2. MCP:
   - `rm /etc/systemd/system/hermes-rs.service.d/97-omfix-mcp.conf`
   - `cat /var/backups/omfix-20261029-20261029130707/config.yaml > /opt/galion/hermes/config.yaml`
   - `systemctl daemon-reload && systemctl restart hermes-rs`
3. Freshness sensor:
   - `rm /etc/systemd/system/galion-nerve.service.d/93-omfix-memfresh.conf`
   - `systemctl daemon-reload && systemctl restart galion-nerve`
4. Hermes memory:
   - `rm /etc/systemd/system/hermes-rs.service.d/96-omfix-memory.conf`
   - `systemctl daemon-reload && systemctl restart hermes-rs` (back to b7fbbc78, memory off)
5. Memory binary:
   - `rm /etc/systemd/system/openmind-memory.service.d/91-omfix-5c7bad5f.conf`
   - `systemctl daemon-reload && systemctl restart openmind-memory` (back to aa77864)

## Spend
- All 5 live test chats used `river-flash` (DeepSeek-V4.1-Flash on River). rs-spend shows
  DeepSeek at $0.00.
- Sandbox chats were mock, with no model.
- rs-spend.json:
  - Session sum is $3.5368 at 14:18 +01:00. The baseline was $3.449.
  - Today's `anthropic/claude-sonnet-5.5` entry is $0.0232. It appeared between 14:08 and
    14:18 +01:00. The logs cannot attribute it.
  - Sonnet is the config's `delegation` and `worker` model, so it may come from another job or a
    delegated subtask. If it was mine, this job's spend is ≤ $0.03. Either way, it is far under
    the $0.50 cap.

## Stack overview (14:20 +01:00)
| Part | State | Notes |
|---|---|---|
| console | console.service, console-rust-primary active | not changed |
| cortex | gaia-cortex active | not changed |
| nerve | galion-nerve f66974c active | + memory-freshness sensor. Overall "degraded": galion-one-mouth degrading, cloudflared failing-soon, tuner-quick/voice-worker absent (not caused by this job) |
| hermes | hermes-rs 5c7bad5f active | memory on, MCP on (8 tools), new SOUL |
| memory | openmind-memory 5c7bad5f active | fresh. L1 extraction missing, so auto-recall hits are 0 |
| vault | build-session, read-only to Hermes | SEC-3 world-writable files (report only) |
| sandbox / gate | transient seccomp unit used; hermes-gate binary appeared from another job | gate workflow not adopted yet |
| backup | galion-backup.timer present; omfix backup folder | off-host continuity is PR #14 (open) |
| CI | blocked (no runner) | must be fixed before any merge |

Missing / next steps:
1. Unblock GitHub Actions, then merge #15, #16, #17 after green CI. Shrink hermes-gateway by
   8.4 KB first.
2. L1 extraction, or an L0 fallback in `/recall`, so automatic recall hits.
3. SEC-3 `chmod o-w`, after Mate's OK.
4. The security contact address from Mate.
5. Decide on the duplicate branches from the other job.
6. Adopt the hermes-gate workflow.
7. Consider a River model for `delegation`.

## How to run the demo (console → Hermes page)
1. Pick the model **river-flash** (free on River). Start a new chat.
2. Save: "Remember this for later: my demo codeword is blue-harbor-42. Save it to memory."
   Hermes calls `memory_save`. Turns are also captured automatically.
3. Recall: start a **new** chat and ask "What is my demo codeword? Check your memory."
   Hermes calls `memory_search`, finds the earlier turn and answers.
4. Vault read: "List the notes in the vault and read 02-goals.md. Give me its title and the
   first goal." Hermes calls `vault_list` and `vault_read`.
5. Optional: "Search the vault for 'storage'" (`vault_search`).
6. Write test: "Write a new note into the vault." Hermes should refuse, because the vault is
   read-only.
