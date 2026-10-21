<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Sector plan: `vps-ops`

Written 2026-10-20 20:41-21:05 CEST (+02:00), Europe/Zagreb. This is a plan only: no code, no commits, no builds, no VPS writes.
VPS access was read-only: two ssh sessions at 20:42:23 and 20:42:35 CEST running `systemctl list-units/list-timers/show/is-active`, `ss -ltn(p)`,
`git --no-optional-locks status/rev-parse/tag/remote/log`, `stat`, `sha256sum`, `ps`, `/proc/<pid>/{cgroup,exe}` and loopback GETs.
**Disclosure:** the second session also ran `/opt/galion/rust/bin/hermes-gateway --version` to test for a non-binding version flag. The binary
did not print a version. It ran its startup check, printed `hermes-gateway refusing to start: API_SERVER_KEY is missing or shorter than 16 chars`
and exited without binding. No state changed, but this went beyond pure reads. The finding feeds S2.2 and risk 3.
Sources: PLANNER-BRIEF.md, RULES.md, WAVE2.md, CALL-RECORD.md, CALL-RECAP.md, review-20261020/LIVE.md, and integration-1 (8fd26f6) docs:
cutover/{tuner,room,desk}.md, cutover-approval.md, cutover-units.md, staged-not-live.md, voice-mouth-503.md, console-prod.md, host-decisions.md,
survival-audit.md, dist-age.md, binary-vs-source.md, real-tests/{nerve,survival-heal,desk-linkwatch}.md, integration/integration-1.md,
launch/GATES-OCT31.md, memory-api.md. Also read: the running slot worktrees wt/console-static (slot 34) and wt/tunnel-audit (slot 35).
Launch deadline: Oct 31. CEST ends Oct 25 03:00, after which stamps are CET (+01:00).

## 1. Sector scope
VPS <vps> operations:
- the live units and binaries
- one-service-at-a-time cutovers: tuner, room, desk, voice 8003 + One mouth 8002, nerve, survival, console, link-watch
- the cloudflared ingress (dead routes) and the `tuner-quick` quick tunnel
- unowned listeners 8431-8433, 8652 and 8653
- the frozen working copy /opt/galion/openmind
- deploy tooling (galion-deploy), version tagging and build-commit stamping

Out of scope, owned by other sectors:
- nerve/cortex logic (nerve and cortex plans)
- merging branches to main (integration-ci)
- SEC-1..4 secrets and permissions (security)
- hermes-rs and openmind-memory replacement (memory schema is an owner decision, memory-real.md:100)

**Every VPS change below is an owner-approved step.** It covers one service at a time, with a rollback written down before the step runs.
Agents prepare, rehearse on the box, and verify with read-only probes. The live step is run by Slavik, or by one operator agent that he
approves by name for that single step (RULES.md:9, rule 4).

## 2. Current state (read-only probe 2026-10-20 20:42 CEST unless cited)
| Item | State | Evidence |
|---|---|---|
| Working copy | /opt/galion/openmind HEAD **2f5a730** (Oct 20 10:44:56 +02:00, "ONE-PROBE"). **17 porcelain lines, all untracked**: 15 `*.bak-20261018-*`, `dist/bin-ws/`, `stage/`. **0 tags.** **1 remote now (`origin`)**. LIVE.md:14 (17:50) said there was none; the URL was not printed | probe; LIVE.md:14,36 |
| Divergence | VPS main and GitHub main have diverged (+141 / 52 vs 74d52ec). Main is now 13a296c. Reconciling them is integration-ci's job, not done here | LIVE.md:14 |
| Running units | 12 project units: cloudflared, console, galion-desk, galion-health, galion-nerve, galion-one, hermes-rs, link-watch-rs, openmind-memory, tanga-proto, tuner, tuner-quick. Adding console-getty (a container getty) gives the "13" in CALL-RECORD:10 / CALL-RECAP:38. Inactive: link-watch, hermes (both replaced) | probe |
| Load | 7.89 / 7.51 / 6.61 on 8 cores (Ryzen 7 3800X, 16 GB, no GPU) | probe; voice-mouth-503.md:16 |
| Backups | galion-backup.timer: last 20:22 CEST, next **Oct 21 02:20 CEST**, about every 6 h | probe (VPS clock UTC, converted) |
| Deploy tool | /opt/galion/rust/bin/galion-deploy: bash, 2,830 B, mtime Oct 18 22:13, sha256 `86207af5617720098c82…`. Flow: backup, then `$NEW --version`, install, restart, 20 s health poll, auto-rollback. **Not in the repo** (not in main or integration-1). /var/backups/galion/deploy holds only 2 galion-health backups | probe; cutover/tuner.md:744,798 |
| Versions | All 53 workspace crates inherit `version = "0.1.0"` (Cargo.toml `[workspace.package]`). **No build-commit stamping anywhere** (no GIT_SHA/BUILD_COMMIT/SOURCE_DATE_EPOCH in crates/edge/scripts). Only tuner has a non-binding `--version` (swarm/tuner-cutover 5fd1611). hermes-gateway treats `--version` as a start (see disclosure) | rg in integration-1; probe |

Live binaries (stat at 20:42 CEST; origin from LIVE.md:44-56):
| Unit | Port | Binary | mtime CEST | Built from | Replacement on branch |
|---|---|---|---|---|---|
| galion-one | 8091 + mouth 8002 (**503 "loading", permanent**) | /opt/galion/openmind/dist/bin-ws/galion-one, **inside the frozen working copy** | Oct 18 22:13 | ws/ui box build | one-probe b44cd4f + voice-mouth 5d776c1 |
| galion-desk | 8646 | /opt/galion/bin/galion-desk 4.7 MB glibc | **Oct 10 11:02** | unversioned VPS source; forks python3 desk-hermes.py on every /desk | desk-linkwatch a54c792 |
| tuner | 8650 | /opt/galion/tuner/bin/tuner, sha `ecad72a4…dd25` (re-checked) | Oct 18 19:18 | hand-built /opt/galion/tuner/src | tuner-cutover 5fd1611, artefact sha `47a17df2…1370` |
| galion-nerve | 8670 | /opt/galion/rust/bin/galion-nerve | Oct 18 22:35 | ws/nerve a63b730 (not main) | nerve 5802d0e (openmind-nerve); replay FAIL 61/D |
| galion-health | 8660 | /opt/galion/rust/bin/galion-health | Oct 18 22:14 | ws/foundation | none (not in main) |
| hermes-rs | 8642 | /opt/galion/rust/bin/hermes-gateway | Oct 18 22:00 | ws/switch-gaps | out of scope |
| openmind-memory | 8420 | /opt/galion/rust/bin/openmind-memory | Oct 18 21:41 | oom-target/deploy | out of scope |
| link-watch-rs | - | /opt/galion/rust/bin/link-watch | Oct 18 21:57 | ws/switch-gaps | desk-linkwatch a54c792 |
| tanga-proto | 8671 | /opt/galion/tanga-proto/bin/tanga-serve | Oct 18 22:35 | box galion-one-design | none |
| console | 8080 | node + **Vite dev server**, serves /src/**, package.json, vite.config.ts | - | /opt/galion/console-ide 2169d1c + ~60 dirty | console-static (slot 34, local, uncommitted) |
| cloudflared ×2 | 20241/20242 | vendor, Sep 21 | - | - | - |

Cutover status: `docs/cutover-approval.md` says **"no" for every service**. Nothing from the swarm is deployed (integration-1.md Needs #8).

| Topic | Facts |
|---|---|
| Tuner | Plan is complete: cutover/tuner.md, 21/21 routes byte-identical, reproducible build, galion-deploy path, rollback in §4 |
| Room | Plan is complete: cutover/room.md. It starts a new service on :4020 (nothing listens there today) from staged room.service. It needs an exposure decision (loopback only) |
| Desk | Checklist is complete: cutover/desk.md §5. The shadow run on 18646 writes a /tmp state copy on the VPS, so it needs approval. The drop-in route keeps the working copy untouched |
| Voice / One | voice-mouth-503.md §4. Root cause: no TTS backend; `/health` hard-coded. The fix is a new galion-voice-serve unit on 127.0.0.1:8003 (~215 MB models, MemoryMax 768M, CPUQuota 200%), then a galion-one drop-in (GALION_MOUTH/EAR=8003) plus a new galion-one. Blockers: **SIMD exception** (candle gemm/pulp) and **mind 402** (OpenRouter credit), which keeps a full voice turn failing even after the fix |
| Nerve | Live galion-nerve says PASS while the new build's replay says FAIL 61/D. The live one misses about 59% failed tunnel requests (real-tests/nerve.md:47-61). The deploy-sim agent (slot 33, nerve cutover rehearsal) was stopped by the request limit at 20:38 (CALL-RECORD:42). Its worktree has no work (head 5fd1611) |
| Survival | guardian-tick and the survival drop-ins exist only as staged files (deploy/survival/{console,galion-desk,galion-one,link-watch,tuner}.d, guardian-tick.{service,timer}). WS-SURV shows D on integration-1 but is **not live** (integration-1.md Needs #3). Box heal capped at 3/3 per hour; live classify is dry-run only (survival-heal.md). galion-one's fragment still says "staged, not installed" and has no [Install], yet it is enabled through a symlink (survival-audit.md) |
| Cloudflared | B66 lists **33 ingress hostnames with no listener** (host-decisions.md). The nerve capture saw **13 of them failing on 10 ports** (:3004 :3005 :3010 :3011 :3020 :4000 :4001 :4003 :4020 :4060) at 14 failed req/min (59%). Top refusals: :4001 and :3011 at 180 each. The difference between 33 and 13 is configured versus seen with traffic. Note :4020 is both a dead ws3d ingress and room's default bind. **Room cutover collision**: once room listens on 4020, ws3d.app/workspace3d.app would route public traffic to room. Fix: ingress cleanup first, or move room's bind |
| tuner-quick | Transient unit (/run/systemd/transient), active since Oct 18 19:19:30 CEST. Random trycloudflare URL publishes 8650. Does not survive a reboot. This is SEC-5 (GATES-OCT31.md:33). tuner.galion.app has an ingress rule but is NXDOMAIN |
| Tunnel-audit (slot 35) | Running. Local, uncommitted crates/tunnel-lint (lib.rs 20:41) + scripts/real/tunnel-audit.sh |
| Unowned ports (all started by hand in root ssh session scopes, ppid 1, no unit; VPS ps times converted to CEST) | 8431 openmind-memory pid 1375169 (Oct 18 22:14). 8432/8433 `/root/growth-20261018/bin/openmind-memory.new` **(deleted)** pids 1412682/1428048 (22:23, 22:24). 8652 hermes-gateway `--mock` shadow pid 1395619 (22:15, RSS 53 MB). 8653 `/root/growth-20261018/bin/hermes-gateway.new` pid 1443105 (22:32, RSS 30 MB). Sources: memory-api.md:124-128, ws-hermes.md:13 |
| Console | Vite dev on 8080 serves source files: `/src/lib/host.ts` 200, `/package.json` 200 (console-static.md, 20:28). The nerve saw scanner probes. console-static (slot 34, running, uncommitted) has a Rust front on 8080 and moves the console to 8088 (phase 1). It needs the owner to commit the ~60 dirty console-ide files first |
| link-watch | link-watch-rs is active with its France URL set. The old link-watch.service (galion-light script) is inactive. The France probe config is "awaiting Slavik" (cutover-approval.md). realtest live has a known-fail for "link-watch.service inactive" that tests the wrong unit (CALL-RECORD:31) |

## 3. Sessions
Agent counts: **target** is what the 250-ceiling / ~50-per-group model would assign. **Useful max** is how many can work without colliding.
**No vps-ops task can use 50 agents.** The limits are:
- Live changes are serialised to 1 at a time by policy.
- There is one VPS at load ~7.9 on 8 cores, and every extra ssh prober adds to it. Cap: **2 concurrent VPS probers swarm-wide**.
- There are about 12 deployable binaries.
- The bottleneck is Slavik's approvals, not agents.

All box work follows RULES.md: own slot ports, release builds, `docs/real-tests/<slug>.md`, Zagreb stamps.

### S1: Baseline and provenance (Oct 21)
Goal: one signed-off record of what runs, from which bytes, and how each piece rolls back. This is the "before" picture for every cutover.

| Task | Target | Useful | Parallel units | Inputs / deps |
|---|---|---|---|---|
| S1.1 VPS ledger `docs/ops/vps-baseline.md`: per unit, ExecStart, drop-ins, Restart, enabled, sha256, mtime, listener, health code, journald/log path. Read-only, one ssh batch | 2 | 1 | none (one prober) | this plan §2 |
| S1.2 Binary provenance: rebuild candidate commits on the box (ws/ui, ws/foundation, ws/nerve a63b730, ws/switch-gaps, the tanga-serve source) and compare sha/BuildID/strings with the live bytes. Settles "no source in main" for galion-health/nerve (CALL-RECAP Needs 6) | 10 | 4 | per binary (10). CPU-bound, so 4 | S1.1. Branch refs in base.git |
| S1.3 Bring galion-deploy under version control: copy its text (2,830 B, sha 86207af5…) into `deploy/galion-deploy` on a swarm branch. Add a box test harness with a `systemctl` shim (the box has no systemd) | 2 | 1 | single script | read-only `cat` of the script |

Success:
- Every live binary has a provenance row: matching commit, or "no reproducible source" with evidence.
- galion-deploy's test reproduces its four outcomes on the box: deployed; rolled back ok; ROLLBACK ALSO UNHEALTHY; `--version` hang with an old binary.

Slavik approves: none (read-only). He confirms the ledger as the baseline.

### S2: Version and build-commit stamping (Oct 21-23)
Goal: every deployable binary answers `--version` with `name 0.1.0 (commit 13a296c[-dirty], built 2026-10-22T10:00:00+02:00, x86_64-unknown-linux-musl)` **without binding or starting**.
Release artefacts carry SHA256SUMS and BUILD-INFO. There is a written tag plan.

| Task | Target | Useful | Parallel units | Deps |
|---|---|---|---|---|
| S2.1 `galion_common::build_info`: values come from `option_env!` (`GALION_BUILD_COMMIT`, `GALION_BUILD_TIME`, set by the release script). No build.rs git calls, which keeps builds reproducible. Defaults to "unknown" | 1 | 1 | one module | common 26d98b0 merged |
| S2.2 Non-binding `--version`/`--check`/`--help` per binary, the tuner pattern. Binaries: galion-one, galion-desk, desk-hermes, link-watch, openmind-nerve, hermes-gateway (today it **runs startup checks on `--version`**, as observed), openmind-memory, room, galion-voice-serve, guardian-tick, galion-console-static, tuner (add stamp). **No new HTTP fields**, so served bytes stay byte-identical for the route diffs | 12 | 6 | per crate (disjoint files). Cargo.lock untouched | S2.1 |
| S2.3 `scripts/release/artifacts.sh`: clean checkout of a merged main SHA, x86_64 (+aarch64) musl, double build to prove reproducibility, `SHA256SUMS`, `BUILD-INFO.txt` (commit, toolchain, flags; no RUSTFLAGS, see hades rustflags note). Output to /workspace/swarm/artifacts/<sha>/ | 2 | 1 | single script | S2.1 |
| S2.4 Tag and version policy memo: annotated `v0.1.0` on the merged launch commit (tag ruleset `v*`, SCORECARD-READINESS). Service bumps 0.1.x per cutover are optional. CHANGELOG from integration-1 merge table. **Agents create no tags** (tags go out through the push path, which is owner-only) | 1 | 1 | doc | none |

Success:
- On the box, `<bin> --version` exits 0 within 100 ms and `ss` shows no new listener, checked for every binary by the real script.
- Two builds of one SHA give identical sha256s.
- `galion-deploy` (S1.3 harness) accepts each new binary.

Slavik approves: the tag scheme, and creating/pushing `v0.1.0` (his action).

### S3: Box rehearsal (deploy-sim relaunch) (Oct 22-23)
| Task | Target | Useful | Parallel units | Deps |
|---|---|---|---|---|
| S3.1 Rehearse each cutover on the box with the real galion-deploy and a systemd shim. Scenarios: healthy; unhealthy then rollback; double failure; port busy; old binary ignores `--version`. Services: tuner, room, desk + desk-hermes, voice-serve, galion-one, nerve, guardian-tick, console-static | 8 | 4 | per service, own slot ports | S1.3, S2.2 |
| S3.2 `scripts/ops/{preflight,postflight}-<svc>.sh`: GET/stat/sha256 only, each check one read, no loops, no POST. Encodes each go/no-go list (e.g. tuner live sha still ecad72a4…, backup timer not due within 5 min, port free for room) | 7 | 4 | per service | S1.1 |

Success: every service shows all 5 scenarios ending in the documented state on the box. Pre/postflight scripts pass against the live VPS read-only (preflight) and against box instances (postflight).

Slavik approves: none (box only, read-only VPS).

### S4: Cutovers, one service per approval (Oct 22-29)
Each task: a prep agent rebuilds from **merged main** (SOURCE-OF-TRUTH rule 6), refreshes the go/no-go with today's shas, and stages the exact commands and rollback.
Then Slavik approves *that* service in `docs/cutover-approval.md`. Slavik or one named operator agent runs it. Postflight and `galion-realtest` live and nerve replay are read-only.
A 24 h watch follows (RSS, restarts, nerve rows).
**Never two in one session. A failed check means rollback, not retry.**

| # | Task (order = risk, lowest first) | Target | Useful | Rollback (pre-written) | Specific gates |
|---|---|---|---|---|---|
| S4.1 | tuner 8650 binary swap via galion-deploy (cutover/tuner.md §3) | 1 | 1 | auto, then `galion-deploy tuner <backup>`; sha back to ecad72a4… | tuner-cutover + common merged; 21/21 diff after the switch |
| S4.2 | room: new unit on loopback | 1 | 1 | stop, remove unit, daemon-reload (cutover/room.md) | **:4020 is still a ws3d ingress target**: S5.2 must land first, or room binds elsewhere. Sensitive-file check of the read roots |
| S4.3 | galion-desk + Rust desk-hermes (drop-in `DESK_HERMES_BIN`, no write into the working copy) | 1 | 1 | `.oct10` copy restored + restart (desk.md step 13) | approved VPS shadow on 18646 with /tmp state copy; mindpack numbers equal |
| S4.4 | galion-voice-serve: **new** unit 127.0.0.1:8003 + models in /var/lib/galion/voice-models | 2 | 2 (build/models + unit/limits) | `systemctl disable --now galion-voice` | **SIMD exception**; ~215 MB disk; MemoryMax 768M with load ~7.9 |
| S4.5 | galion-one: new binary at /opt/galion/rust/bin/galion-one via a drop-in that overrides ExecStart (galion-deploy resolves TARGET from ExecStart, so without the drop-in it would install into the working copy dist/bin-ws) + drop-in GALION_MOUTH/EAR=8003 | 1 | 1 | remove drop-in; galion-deploy backup | S4.4 healthy 24 h; 8002/health 200; nerve galion-one-mouth healthy; **mind 402 remains** (owner credit decision) |
| S4.6 | nerve: galion-nerve 8670 to openmind-nerve 5802d0e. First an **observe-only shadow on a spare loopback port** (approved new process), then the swap | 2 | 1 | galion-deploy backup of the Oct 18 build | nerve sector's plan; expect the dashboard to go from PASS to FAIL (honest); galion-health/console consumers of 8670 checked |
| S4.7 | survival: guardian-tick.timer in **classify/dry-run** mode first, then the Restart=always drop-ins one unit at a time, then apply-allowlisted after a week | 1 | 1 | disable timer; remove the drop-in, daemon-reload | WS-SURV becomes truly live; heal cap 3/h; Hades token custody (gaia plan S3) |
| S4.8 | console phase 1: Rust static front on 8080, console moved to 8088 | 2 | 1 | disable console-static, remove drop-in, restart console | slot 34 committed/pushed; **owner commits the ~60 dirty console-ide files**; Host-gate parity (401/26 B) |
| S4.9 | link-watch: France probe config approval; retire the inactive link-watch.service file; fix the realtest known-fail to test link-watch-rs | 1 | 1 | restore unit file copy | desk-linkwatch merged |

Success (each): postflight green; `galion-realtest` live is no worse than before (41/43 now). The nerve shows the targeted row healthy (mouth, edge). No unrelated port changes (`ss` diff limited to the target).

Slavik approves: each row separately by name; SIMD exception (S4.4); OpenRouter 402 (S4.5 full voice turn); console-ide commit (S4.8); any VPS shadow process (S4.3, S4.6).

### S5: Edge: cloudflared and tuner-quick (Oct 22-27)
| Task | Target | Useful | Parallel units | Deps |
|---|---|---|---|---|
| S5.1 Finish tunnel-audit (slot 35, running): `tunnel-lint` over a **redacted copy** of the ingress list (no credentials file read). Joins hostname, port, listener, B66 decision and nerve traffic. Reconciles 33 configured vs 13 failing | 1 | 1 | running agent | slot 35 |
| S5.2 Exact ingress diff: remove the dead hostnames, keep one/console/obsidian/tuner. Validate on the box with `cloudflared tunnel ingress validate/rule` against a copy. Owner runs it: back up config.yml, apply, restart cloudflared.service. Rollback: restore the copy and restart. **The restart blips every public hostname (one.galion.app too)**: pick a quiet window. DNS record removals are a separate owner list (no DNS changes by agents, RULES 5) | 2 | 1 | single config | S5.1 |
| S5.3 SEC-5 tuner-quick memo with three options: (a) stop the transient unit (no rollback needed; the URL is random anyway); (b) replace with a named route tuner.galion.app (needs a DNS record) via staged tuner-publish.service (binary on ws/one-desk-tuner, not main); (c) keep until launch. Covers the hostname mention in session-2026-10-18-evening.md:45 | 1 | 1 | doc | none |

Success: after S5.2, nerve replay and live show 0 refused cloudflared upstreams and the `edge` incident closed. one.galion.app 200, /tanga/ 200 and console 302 are unchanged. After S5.3 is applied, no trycloudflare process remains (or option (c) is recorded).

Slavik approves: the ingress edit + cloudflared restart; DNS removals; the SEC-5 option.

### S6: Strays, working copy, unit hygiene (Oct 22-28)
| Task | Target | Useful | Parallel units | Deps |
|---|---|---|---|---|
| S6.1 Per-port dossier for 8431, 8432, 8433, 8652, 8653: established connections sampled read-only 3× over 24 h; any config/unit naming the port (rg on /etc/systemd, deploy/, galion-light configs, **not** secrets dirs). Then kill proposals, one port per approval: lowest risk first (8432/8433 deleted binaries), then 8653, 8431, 8652. Rollback: 8431/8652 relaunch from the recorded cmdline. **8432/8433/8653 cannot be relaunched** (binaries deleted or in /root/growth-20261018); say so | 5 | 2 | per port; 1 VPS prober | S1.1 |
| S6.2 Stop executing from the working copy: after S4.5 nothing runs from dist/bin-ws. Then propose archiving the 15 `.bak` files + `stage/` to /var/backups/galion/openmind-untracked-<stamp>.tar (never `git clean`). The working copy itself stays at 2f5a730 untouched until integration-ci reconciles. Record the new `origin` remote | 1 | 1 | doc | S4.5 |
| S6.3 Unit-file hygiene proposals: galion-one fragment comment + missing [Install]; leftover link-watch.service/hermes.service; deploy/staged/tuner.service pointing into the working copy (cutover/tuner.md:856); deploy/console-prod.service stale comment | 1 | 1 | doc | S1.1 |

Success: 0 listeners outside a unit (`ss -ltnp` cgroups all in system.slice). The nerve "5 unmanaged listeners" vulnerability is gone. `git status --porcelain | wc -l` on the VPS is 0, and HEAD is still 2f5a730.

Slavik approves: each kill; the archive move; each unit-file edit.

### S7: Verification, clock, freeze (Oct 24-31)
| Task | Target | Useful | Parallel units | Deps |
|---|---|---|---|---|
| S7.1 Post-cutover verifier: realtest live + nerve replay + ledger diff after each S4/S5/S6 step. Updates known-failures | 1 | 1 | serial with cutovers | S4 |
| S7.2 CEST→CET check at Oct 25 03:00: VPS clock is UTC; check galion-deploy backup names (+zzzz), guardian/nerve stamps and backup timer times. **No live change on Oct 25 01:00-04:00** | 1 | 1 | none | none |
| S7.3 Docs: LIVE binary table, STATUS rows B21/B51/SEC-5/OPS-1/WS-SURV, cutover-approval.md rows. Rows change only after the owner's approval is recorded | 1 | 1 | single files | S4 |
| S7.4 Freeze + launch runbook: no cutovers after Oct 29 18:00 CET; one-page rollback table for every changed service | 1 | 1 | doc | S4-S6 |

Success: the ledger at Oct 30 matches docs/STATUS.md. Every changed service has a tested rollback line and a backup file listed under /var/backups/galion/deploy.

## 4. Totals
| Session | Tasks | Target agents | Useful max |
|---|---|---|---|
| S1 Baseline/provenance | 3 | 14 | 6 |
| S2 Version + stamping | 4 | 16 | 9 |
| S3 Box rehearsal | 2 | 15 | 8 |
| S4 Cutovers (prep; execution is serial) | 9 | 12 | 10 |
| S5 Edge / tunnels | 3 | 4 | 3 |
| S6 Strays / hygiene | 3 | 7 | 4 |
| S7 Verify / clock / freeze | 4 | 4 | 4 |
| **Total** | **28** | **72** | **44** |

The ~50-per-task model fits nowhere. The widest task is S2.2 (12 target, 6 useful, per crate). Live execution is always 1.

### Realistic schedule at 16 concurrent (box-wide); vps-ops gets 3, at most 2 of them touching the VPS
| Window (Zagreb) | Agents | Work | Live step (needs owner) |
|---|---|---|---|
| Oct 20 eve | 2 | slot 34 console-static and slot 35 tunnel-audit finish (already running) | - |
| Oct 21 | 3 | S1.1, S1.2 (×2), S1.3, S2.1, S2.4 memo | S6.1 kill 8432/8433 (deleted binaries) |
| Oct 22 | 3 | S2.2 (×2 batches), S2.3, S3.2 | **S4.1 tuner** (after main merge) |
| Oct 23 | 3 | S3.1 (×2), S5.2 diff, S5.3 memo | S5.3 tuner-quick decision; S6.1 8653 |
| Oct 24 | 3 | S4.3/S4.7 prep | **S5.2 ingress cleanup** (before room); S4.2 room afterwards if time allows |
| Oct 25 (CET switch 03:00) | 2 | S7.2 clock check | none 01:00-04:00; **S4.3 desk** in the afternoon |
| Oct 26 | 3 | S4.4/S4.5 prep, S4.6 shadow prep | **S4.4 voice 8003** (if SIMD approved); S4.7 guardian-tick dry-run |
| Oct 27 | 3 | S4.6 shadow analysis | **S4.5 galion-one**; S6.1 8431/8652 |
| Oct 28 | 3 | S4.8 prep, S6.2/S6.3 | **S4.6 nerve swap** |
| Oct 29 | 2 | S7.1, S7.3 | **S4.8 console** (if console-ide committed); S4.9 link-watch; freeze 18:00 |
| Oct 30-31 | 1 | S7.4 runbook, final ledger | none |

That is about 30 agent-runs. Every live step avoids ±10 min around galion-backup (about 02:20 / 08:20 / 14:20 / 20:20, re-read from list-timers before each step).
If an approval slips a day, later cutovers move or drop. Suggested drop order: S4.8 console, then S4.6 nerve, then S4.4/S4.5 voice.

## 5. Items needing Slavik (sector)
1. Merge integration-1 (and tuner-cutover/common/desk-linkwatch/room/voice-mouth/one-probe/nerve within it) to main. Every cutover deploys only from merged main.
2. Per-service approval rows in docs/cutover-approval.md (S4.1-S4.9), one per day at most.
3. SIMD exception for candle (voice), and the OpenRouter 402 decision (One's mind step).
4. VPS shadow processes (desk on 18646, nerve observe-only on a spare port) and the /tmp state copy.
5. Cloudflared ingress edit + restart; DNS record removals; :4020 conflict between ws3d ingress and room.
6. SEC-5 tuner-quick: stop, replace with named route, or keep.
7. Killing each stray listener (8431-8433, 8652, 8653).
8. Archiving the working copy's untracked files; any unit-file edit; guardian-tick install and later apply mode.
9. Tag scheme and creating `v0.1.0` (his push).
10. Committing the ~60 dirty console-ide files (prerequisite for S4.8). Node on the VPS for console phase 2.
11. Who runs live steps: Slavik by hand, or one named operator agent per step.

## 6. Top risks
1. **Approval and merge bottleneck.** There are about 10 live steps and at most 1/day. Nothing is merged to main yet. 9 days to Oct 31 leaves zero slack.
2. **Edge blast radius.** A cloudflared restart blips every public hostname. Room on :4020 would be exposed through the dead ws3d ingress if it lands before the ingress cleanup.
3. **galion-deploy assumptions.** It is not in the repo and untested on the box. It runs `$NEW --version`: old binaries bind, and hermes-gateway runs startup checks (observed). A wrong rollback path on a binary inside the working copy would write into /opt/galion/openmind.
4. **Voice/One stays degraded even after a correct cutover.** Mind 402 (credit), the SIMD exception and 215 MB of models on a box at load ~7.9/8, plus a 768 MB memory cap, on 16 GB shared with Vite (~505 MB).
5. **Irreversible cleanups and state drift.** 3 of the 5 stray processes run deleted or ad-hoc binaries and cannot be relaunched. The VPS main has diverged from GitHub and now has a remote. The CEST→CET switch lands mid-schedule (Oct 25).
