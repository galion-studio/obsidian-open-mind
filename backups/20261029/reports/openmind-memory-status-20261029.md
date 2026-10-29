# Hermes memory: new implementation status (t374u, scope narrowed by t375u/t377u/t379u)

Written 2026-10-29 ~12:00 CET (Europe/Zagreb, UTC+1). READ-ONLY: nothing was changed, pushed, restarted or deleted.
Scope: only #2, the NEW Hermes memory implementation (other agents cover the vault features, the Hermes<->Obsidian integration and tests/CI/deploy).
No secrets or VPS address in this file. VPS times were read in UTC and converted to CET/CEST here.

## 0. Sources and what I could not read
- Transcripts: ReadTranscript only reached my own run. Other sessions (Samantha, New Bot, New Bot 1) need a session id I was not given (`in=parent` returned "no session ... has the id"). So I have **no verbatim owner quote** of the "new memory implementation" request. Owner-side facts below come from RecallMemory journal entries and /workspace/swarm/CALL-RECORD.md.
- GitHub via cursor-github (read-only): get_branch, compare_commits, list_pull_requests, search_issues, get_file_contents.
- Box: /workspace/swarm/CALL-RECORD.md, plans/wave4-tasks.md, reports/*, the Page "Gaia v2 — where we are".
- VPS: `ssh slavik-vps`, only systemctl show/is-active, ss, curl loopback /health, ls/stat, grep of key *names*, the binary usage line. No git in /opt/galion/openmind; link.env not read.

## 1. What the owner asked for (dated)
- 2026-10-28 ~13:41 CET: slot 388 found memory stale and opened owner decision **D-388-1 "restore a writer"**: (a) hermes-rs memory client (recommended), (b) freeze read-only, (c) retire openmind-memory. Also D-388-2 (archive the old root vectors.db), D-388-3 (freshness alert), D-388-4 (fix the record).
- 2026-10-28 14:24 CET refill: slot 403 hermes-rs-memory-client ("D-388-1 prep, recommended option, owner still decides"), 405 memory-freshness-sensor, 406 legacy-vectors-archive-plan.
- 2026-10-28 18:38 CET refill: 426 hermes-memory-session-scope (stop cross-session leak, F-403-1).
- 2026-10-28 23:08 CET refill: 442 hermes-gateway-candidate-2, 443 memory-deploy-candidate (rows only, **never started**; 23:45 record: "Running 0/18, waiting 436-449").
- 2026-10-29 07:55 CET (t337u, journal): the owner's queue includes **"memory saving"** (after voice, dead web addresses, backup check, phone alerts). 07:56 (t338u): voice paused; memory saving still queued.
- 2026-10-29 (pt374u, from the parent): "the user asked a couple hours ago for a new implementation of the memory for the Hermes research and the whole Obsidian stack". Not found verbatim in readable transcripts.
- Rules for this work (2026-10-29, t377u/t379u): test everything in the sandbox first using live-style deployments; Hermes may change live only after security checks pass; deep reasoning on higher OpenRouter models, routine work on cheap ones.

## 2. Design (what "new memory" is, from docs on the branch)
Source: `docs/hermes-memory-client.md` and `docs/ops/memory-db-stale-diag.md` on
https://github.com/galion-studio/obsidian-open-mind/tree/swarm/hermes-memory-session-scope

- **Why:** the only regular writer to openmind-memory (:8420) was the Python Hermes `memory_tencentdb` plugin. It retired 2026-10-18 21:42 CEST, and hermes-rs has had no `/recall` or `/capture` since then.
- **Client** `crates/hermes-rs/src/memory.rs` (841 lines): `POST /recall` before each turn (3 s timeout) adds a fenced `<memory-context>` block to the last user message (max 8000 chars); `POST /capture` runs in the background after each completed run (10 s timeout, max 4 at once, extras dropped and counted). Bearer key from `MEMORY_TENCENTDB_GATEWAY_API_KEY`, else `TDAI_GATEWAY_API_KEY`, never logged. Memory never blocks a chat: errors are counted, plus a nerve `memory_error`.
- **Safety:** URL must be loopback http, or the gateway exits 2 at start. Replies capped at 256 KiB. No new dependencies.
- **Switches:** `HERMES_RS_MEMORY` (off by default), `_URL`, `_USER`, `_RECALL_MS`, `_CAPTURE_MS`, `_CROSS_SESSION`; YAML `hermes_rs.memory.*`. The old `memory.provider: memory_tencentdb` keys are ignored on purpose.
- **Session scope (slot 426):** `/recall` takes `scope: session|all`, and the server echoes it. The client defaults to `session` and **fails closed**: if the reply has no echo (older server), recall is dropped and `recall_unscoped` is counted. So **openmind-memory must be deployed before or with the gateway.**
- **Freshness tool:** `openmind-memory freshness <data_dir> [instance] [max_age_h]` is read-only and exits 3 when stale. The nerve sensor (slot 405) alerts on exit 3. It is off by default.
- **Local time (slot 367):** stored times stay UTC; search results show Europe/Zagreb time.
- **Not ported (D-403-3):** query-mmd second recall, offload/ingest of whole sessions, session-end flush, memory_tencentdb tool calls. `POST /v2/offload/compact` returns 501 (plans/research-context-compaction.md).

## 3. What exists

### Branches (galion-studio/obsidian-open-mind), all pushed, none merged, no PR
`compare main...swarm/hermes-memory-session-scope`: **diverged, 8 ahead, 120 behind** main (merge base 3f015986). Open PRs: none for memory (#14 off-host, #7 brand, dependabot). Issue search "memory": 0 results.

| Commit | Branch | What |
|---|---|---|
| 9dda6146 | (in memory-db-stale-diag) | search results in Zagreb time |
| 1d7164b4 | [swarm/memory-db-stale-diag](https://github.com/galion-studio/obsidian-open-mind/tree/swarm/memory-db-stale-diag) | freshness command + diagnosis |
| 1010eb7c | [swarm/memory-freshness-sensor](https://github.com/galion-studio/obsidian-open-mind/tree/swarm/memory-freshness-sensor) | nerve sensor, off by default |
| 821417e8, 8887a0cb | [swarm/hermes-rs-memory-client](https://github.com/galion-studio/obsidian-open-mind/tree/swarm/hermes-rs-memory-client) | client + real test |
| ccca5545 | [swarm/hermes-memory-session-scope](https://github.com/galion-studio/obsidian-open-mind/tree/swarm/hermes-memory-session-scope) | scoped recall; **this is the head with everything** |

Older memory branches: swarm/memory-real 723b46c, swarm/memory-schema 7952626, swarm/memory-schema-mergefix 1919ca8, ws/memory 900d465.
Files on the head: hermes-gateway/src/main.rs + tests/real_memory_client.rs and real_memory_session_scope.rs; hermes-rs/src/{api,config,lib,memory}.rs; openmind-memory/src/{engine,freshness,lib,main,store,text}.rs + tests (golden, local_time, real_freshness, real_local_time); zagreb-time; docs (hermes-memory-client.md, memory-api.md, ops/, real-tests/, third-party/).

### Box tests already run (CALL-RECORD)
- 403 (17:39 CET Oct 28): real test against a throwaway openmind-memory passed 3 times; the base negative control failed as expected; workspace 2027 pass / 0 fail; x86_64 and aarch64 musl built.
- 426 (21:59 CET Oct 28): real_memory_session_scope passed 4 times (2 sessions, 0 leaks); negative controls fail "LEAK A->B" as expected; workspace 2032/0/18; size +0 to +4 KB.
- 405: freshness sensor passed on the box and on a VPS throwaway copy run as nobody.
- None of this was a live-style sandbox deployment (full unit, drop-in, swap and rollback rehearsal). Slots 442/443, which would do that, never started.

### Live on VPS (read 2026-10-29 11:55 CET)
- `openmind-memory` is active and runs `/opt/galion/rust/bin/openmind-memory-aa77864` (Oct 21 build, older than every branch above). Drop-ins: 50-rust-bin, 60-nerve-url. Up since 2026-10-22 23:46:47 CEST, NRestarts 0, 127.0.0.1:8420. `/health` 200: `read_only:false, fts:true, embeddingService:false`.
- The live binary has **no `freshness` command** (its usage line lists only serve and snapshot) and **no scope support**.
- Instance DB `/var/lib/memory-tencentdb/instances/galion/vectors.db`: modified 2026-10-18 23:42:30 CEST; WAL 2026-10-19 14:57:55 CEST. **No new memory for ~10 days.** 0 established connections to :8420.
- `hermes-rs` is active and runs `hermes-gateway-b7fbbc78` (built from survival-s47-install; **no memory client**). Drop-ins: 50-rust-bin, 60-nerve-url, 70-research-budget, 90-deploy-t291u. No `HERMES_RS_MEMORY*` names in the drop-ins or `/opt/galion/hermes/.env`. `/health/detailed` returned `{"error":...}` without auth; not pursued.
- `/opt/galion/hermes/config.yaml` (and the `/root/.hermes` symlink, which points to /workspace/hermes) still says `memory.provider: memory_tencentdb`, `memory_enabled: true`, `user_profile_enabled: true`. This setting is no longer used.
- Hermes file memory: `/root/.hermes/memories/MEMORY.md` (3124 B, modified 2026-10-27 19:23 CET) and `USER.md` (1280 B, 2026-10-18 23:41 CEST) are separate from :8420. I did not check whether hermes-rs reads them.
- Nerve runs `openmind-nerve-f66974c` (its line carries the memfresh commits via 674482f1). No memory/freshness env names in its 9 drop-ins, so the sensor is off, and it couldn't run anyway without a memory binary that has `freshness`.
- Gaia v2 Page "Memory" = 1/3: Service running (yes); New memories saved (no, last real save Oct 18); Alarm if it stops (no). D-388-1 is "Waiting for the Hermes update".

## 4. Open owner decisions
- D-388-1: restore the writer with the hermes-rs client (recommended), versus read-only or retire.
- D-403-1: turn it on live (deploy + drop-in + key-match check).
- D-426-1: session scope as default, cross-session only by opt-in. D-426-2: deploy memory before or with the gateway. D-426-3: count every unscoped reply.
- D-403-3: port query-mmd / offload / session-end flush now or later. D-403-4: memory key = session_key, else run id.
- D-388-2 / D-406-x: archive the legacy root `/var/lib/memory-tencentdb/vectors.db` (4 rows, Oct 13).
- D-388-3 / D-405-1/2/3: freshness alert on, 24 h threshold.
- Whether the "new implementation" means only this Rust client, or also a bigger V2 (embeddings are off on live; `/v2/offload/compact` is 501).
- Merge path: the branch is 120 behind main and has no PR.

## 5. Checklist to 100% memory
Each item needs a live-style sandbox deploy that passes the security checks before Hermes touches live. Model tier: "none" means no LLM; "cheap" is a routine OpenRouter model; "high" is a higher OpenRouter model for deep reasoning.

1. [ ] **Owner says yes to D-388-1(a), D-403-1, D-426-1/2** (and D-403-3/4). Sandbox: none. Model: none.
2. [ ] **Rebase or merge ccca5545 onto current main** (120 behind) and open a PR. Sandbox: full gates (fmt, clippy -D, workspace tests, no-python, repo-check, spdx) plus real_memory, real_freshness, real_local_time, real_memory_client and real_memory_session_scope on the rebased head. Model: cheap; high only if conflicts touch hermes-rs api.rs or http.rs.
3. [ ] **Slot 443 memory-deploy-candidate:** reproducible x86_64 musl openmind-memory build (same sha from two paths). Sandbox: live-style deploy of the unit with the same drop-ins on a copy of the live data dir. Check that /health and the GET contract are byte-identical to aa77864, that recall without a scope gives the legacy reply, that `freshness` exits 3 on the copy, and that rollback works (remove 50-rust-bin, restart). Security: loopback only, no new listener, DB integrity check after kill -9. Model: none.
4. [ ] **Slot 442 hermes-gateway-candidate-2:** fold ccca5545 onto the current live gateway line. Note: live is b7fbbc78, not the 20936d52 the row assumed, and the hermes-ui-rebuild/model-picker work since then must be included. Sandbox: memory OFF gives the same behaviour as live (parity P1-P5). Memory ON against the step-3 sandbox memory: 2 sessions with 0 leaks; memory down means chat still returns 200; a non-loopback URL makes it exit 2; the key never appears in logs. Security review of the new code. Model: high for the merge and security review; cheap for test runs.
5. [ ] **Key match check (D-403-1 step 3):** confirm that the key in `/opt/galion/hermes/.env` equals the one in `/etc/openmind-memory.env`, without printing either. Sandbox: test the hash-compare script on throwaway env files first. Model: none.
6. [ ] **Live deploy, memory first (D-426-2):** swap openmind-memory to the step-3 binary only after its sandbox deploy and security checks pass. Sandbox: the step-3 rehearsal. Model: none.
7. [ ] **Live deploy of the gateway with the flag still OFF**, then add a drop-in `Environment=HERMES_RS_MEMORY=1`. Sandbox: the step-4 rehearsal including the drop-in. Verify `/health/detailed` memory counters (`recall_ok`, `capture_ok`, `recall_unscoped`=0) and that vectors.db/WAL modification times move after one chat. Rollback = remove the drop-in and restart. Model: none (one routine chat uses cheap).
8. [ ] **Freshness alert on (D-388-3/D-405-1/2):** nerve sensor drop-in with the 24 h threshold. Do this after step 7, or it fires at once. Sandbox: nerve f66974c plus the sensor on a stale copy and on a fresh copy (alert, then resolved). Model: none.
9. [ ] **Clean up the old config:** remove or annotate the unused `memory.provider: memory_tencentdb` block in config.yaml. Sandbox: Hermes config check (`--check-config` prints `memory=<url>`). Model: none.
10. [ ] **Legacy root vectors.db (D-388-2, slot 427):** archive it and drop it from galion-backup. Sandbox: galion-backup fake-tree test (never creates an empty root DB). Model: none.
11. [ ] **Optional V2 (D-403-3):** query-mmd second recall, session-end flush/offload, `/v2/offload/compact` (now 501), embeddings (now off). Sandbox: a full new design + sandbox cycle. Model: high for design, cheap for implementation and tests.
12. [ ] **Update the Gaia v2 Page:** Memory 3/3 once steps 7 and 8 are verified live. Model: none.
