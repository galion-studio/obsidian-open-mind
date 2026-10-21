# Obsidian Open Mind: master plan (dictated by Slavik, 18 Oct 2026)

> **main.md is the base of everything.**

> **Priority #1 (set 2026-10-18 19:01 CEST (+02:00)):** the GitHub repository and automatic publishing of the OpenMind stack after it self-evaluates (passes its own checks: fmt, clippy, tests, license/secret/no-Python/CJK/Zagreb-timestamp scans). **Deadline: October 31, 2026.** Creating the public repo and the first publish need the user's approval and a GitHub login on the build machine (not set up yet).

**Status:** Created 2026-10-18 18:51 CEST (+02:00); architecture study started. Last updated 2026-10-18 19:01 CEST (+02:00).

## 1. Architecture study

Study how the architecture and the lightweight, Rust-only foundation of the Obsidian Open Mind stack work. Use a couple of agents and sub-agents for the analysis.

## 2. Nervous system

Figure out how to save and access sessions, timestamps, and the analytical system from "the nerve", then activate the nervous system.

## 3. Cortex

Build a whole cortex. Everything already connected to the nervous system, or not yet connected to it, must be created or connected, so that the system knows in real time:

- about vulnerabilities,
- where it is "bleeding",
- about DDoS attacks,
- how it can behave in real time to change the environment of the Obsidian Open Mind stack.

## 4. License

The Open Mind stack is a fluid version of open source, under the MIT license.

## 5. Foundation

Lightweight and efficient on CPU, with the option to add unlimited compute instances from NVIDIA, AMD, or Apple, using the Exo project ([exo-explore/exo](https://github.com/exo-explore/exo), distributed inference across heterogeneous devices) and other open-source tools.

## 6. Success metric: the vending machine benchmark

The system must generate profit from the base amount. The size of the base doesn't matter; the result must be profitable.

## 7. Resource setting

Use only a couple of agents and sub-agents. Don't use too many resources; everything must be CPU-light and efficient.

## 8. NOT APPROVED

> [!WARNING]
> **NOT APPROVED: do not implement.**
> Connecting the Revolut account and giving Hermes access to real money is **NOT approved** and must not be implemented.

## 9. Milestone

By **October 31, 2026**, the Obsidian Open Mind stack must be published: everything live, fact-checked, and timestamped.

## 10. Timezone rule

All timestamps use Slavik's current location timezone: **Europe/Zagreb** (Split, Croatia). That is CEST (UTC+2) now and CET (UTC+1) after 25 Oct 2026.

- Never use server time or the server timezone (the VPS clock is UTC) anywhere.
- Every timestamp in main.md, logs, session records, and the cortex/nerve analytical system must be recorded in Europe/Zagreb time with an explicit offset, e.g. `2026-10-18 18:52 CEST (+02:00)`.

## 11. Galleon Arena

Galleon Arena is a new project: an arena of gladiators, where the gladiators are the personas, workers, and teams inside the system. Hermes is the base of the coding sessions, running on `anthropic/claude-opus-5.5` through OpenRouter. **Honest note:** Hermes's OpenRouter connection is currently broken (API key not present in `/opt/galion/hermes/.env`); fix is pending approval. Existing test agent homes at `/opt/galion/arena/homes/g2-0` .. `g2-3` (each with `SOUL.md` + `TASK.md`) already exist on the box as a starting point. Details and to-dos live in [goals.md](goals.md) under *Project: Galleon Arena*.

## 12. Universal engine

The universal engine is based on **Linux and Rust architecture only**. All agent code lives in the GitHub open-source library under the **Obsidian Open Mind Stack** branding (MIT). See [goals.md](goals.md).

## 13. Automatic recognition as fully open-source and secure

The stack must be automatically recognized and verified as a fully open-source, secure project (OSI-approved MIT LICENSE in every repo, OpenSSF Scorecard / Best Practices badge, REUSE compliance, SBOM, signed releases, SECURITY.md, CI secret scanning). See [goals.md](goals.md).

## 14. Standing preference: always learn files

**Always learn files. Files that we are building.** (added 2026-10-18 19:25 CEST (+02:00))

The agents must always learn from and keep up with the files being built as part of the Open Mind Stack. That means the files under construction, such as main.md, goals.md, admin.preferences, persona files, session docs, and any new file created during the work. Before acting, read the current version of these files, and treat every change to them as new knowledge.

## 15. Status snapshot (2026-10-18 19:54 CEST (+02:00))

- Work is live on the VPS, not only documentation: Hermes's own sessions `openmind-foundation-20261018` and `openmind-release-20261018` ran on the VPS. Foundation tickets T1, T2, T3, T4, T3b, T5, T6 all passed and are committed in `/opt/galion/openmind` git (latest `4f06e8f`, 19:51 CEST): Cargo workspace + MIT, Zagreb-time crate, Galion One/desk/room crates, no runtime Python in ported binaries, staged systemd units and console production build. Nothing is switched into production yet; `STAGED.md` awaits Slavik's approval. Release kit (repo files, site, brand, marketing drafts) reviewed, awaiting approval.
- Hermes on OpenRouter since 19:36 CEST: boss `anthropic/claude-opus-5.5` (reasoning high), subagents `anthropic/claude-sonnet-5.5` (max 3, own sessions), background tasks on free models only, Galion One keep-warm on `claude-haiku-5.5`. Live test on Opus replied OK, about $0.10. Verification running.
- GitHub: no `gh` login on the VPS; repo `galion-studio/open-mind-stack` NOT created yet (connector create returned 403 'Resource not accessible by integration'); 112 files prepared and self-checked locally (fmt, clippy, 9 tests, no Python, no CJK, gitleaks clean); device sign-in requested to create it as private; goes public only after checks pass and approval, deadline 31 Oct 2026.
- Guardian and healer exist in old Galion code (Python: `guardian-tick.py`, `desk_heal.py`, upstream auto_healer) and have not run since 8-12 Oct 2026; investigation of why is running. 'The fort' is only the console Defence page heading.
- Security: plaintext world-readable secret copies found in `galion-light/secrets/persist/`; lockdown in progress (permissions to root-only, encrypted backup, no deletion without approval). Earlier findings remain: root password SSH login, about 55,000 failed logins/week, no fail2ban, firewall off.
- Multi-agent to-do run in progress (priorities: 1 secrets, 2 GitHub repo, 3 guardian/healer, 4 Hermes verification).

## Notes (not dictation, added 2026-10-21T23:55:00+02:00 by the swarm, slot 231)

Everything above this heading is Slavik's dictated text and is left unchanged.
These notes only point to where the current facts live; they need Slavik's
approval to stay (plan item D14) and he may move or delete them.

- **§7 agent count.** Superseded by a dated exception: on 2026-10-20 Slavik
  approved a parallel swarm of 20+ agents on `swarm/*` branches
  (`docs/SOURCE-OF-TRUTH.md:38-47`). On 2026-10-21 the wave-4 routine ran with
  "pace 18/max 18" agents (box file `/workspace/swarm/CALL-RECORD.md:796`). The
  CPU-light rule still holds for the product itself.
- **§8 Revolut.** On 2026-10-20 about 22:56 +02:00 Slavik lifted the design ban
  for the Hermes Rust V2 redesign: Revolut may be designed, mocked, read and
  wired. Moving money (orders, transfers, top-ups) is still not approved (box
  file `/workspace/swarm/RULES.md`, hard rule 5).
- **§15 status.** The snapshot is from 2026-10-18 19:54. Current backlog status
  is `docs/STATUS.md` (89 items, 60.7 % done unweighted, computed by
  `scripts/status-percent.sh`); swarm branches are listed there too. Live VPS,
  read-only GETs at 2026-10-21T23:53:39+02:00: nerve `127.0.0.1:8670` `/healthz`
  ok, `/state` mode live, cycle 9 since 22:56:55, score 66 grade D;
  gaia-cortex `127.0.0.1:8674/health` ok, mode live, LLM advisor on, Hermes
  handoff on, 0 open incidents; `galion-nerve`, `gaia-cortex`, `hermes-rs` and
  `openmind-memory` units active.
- **Repository name.** §15 names `galion-studio/open-mind-stack`; the real
  repository is `galion-studio/obsidian-open-mind` (`README.md:14,134`). GitHub
  `main` is still 13a296c at 2026-10-21T23:50:33+02:00.
