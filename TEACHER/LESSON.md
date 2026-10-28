# LESSON — The OpenMind foundation (teacher: Grok Bot, student/lead: Hermes)

Written 2026-10-18T18:58:00+02:00 (CEST). Base plan: /opt/galion/galion-light/publish/obsidian/main.md.
Findings this lesson is built on: TEACHER/A-foundation.md (copy of the read-only study).

## 1. What the foundation is
Today the "Rust core" is three separate single-file, std-only programs built with bare `rustc`
(Galion One `one`, `galion-desk`, `room`) plus a design crate nobody uses (`openmind-schema`).
No Cargo workspace, no git, no lockfile, no LICENSE file, and the Rust binaries still shell out to Python.

The foundation = ONE Cargo workspace at /opt/galion/openmind (git repo, MIT) that:
- holds the shared types (`openmind-schema`) and a shared time helper (`zagreb-time`);
- builds Galion One, galion-desk and room as workspace crates with IDENTICAL HTTP behaviour;
- replaces the Python helpers (desk-hermes.py, link-watch.py) with Rust binaries giving the same output;
- has a staged (NOT enabled) systemd unit for Galion One and a staged (NOT switched) console production build.

## 2. Why
main.md §5/§7: CPU-light, lightweight, Rust-only foundation. §4: MIT. §9: publish by 2026-10-31 — "fact-checked
and timestamped" needs reproducible builds and one timestamp format. §10: every timestamp in Europe/Zagreb.

## 3. HARD RULES (breaking one = ticket fails, no matter what else is good)
1. **No Python.** Never write, run or add Python. New code is Rust. Do not use `python3 -c` for anything (not even JSON pretty-printing; use `jq` or Rust).
2. **Revolut / real money: NOT approved.** Never implement, never mention as a TODO.
3. **Time:** every timestamp you write (code output, PROGRESS.md, commit messages, logs) is Europe/Zagreb RFC 3339 with offset,
   e.g. `2026-10-18T19:05:00+02:00` (CEST) / `+01:00` (CET from 2026-10-25 03:00 local). The VPS clock is UTC — never print server time.
   In shell use `TZ=Europe/Zagreb date -Iseconds`.
4. **No stray CJK characters** in any file you write. Scan and strip (see CHECKS C7).
5. **CPU-light.** Max 3 parallel subagents. Every build: `nice -n 19 cargo build -j 2 ...`. No polling loops faster than 30 s, no `watch`, no busy loops.
6. **Production safety.** Work ONLY inside /opt/galion/openmind. Do NOT stop/restart/replace hermes, console, galion-desk,
   link-watch, memory-gateway, cloudflared or the running `one` process. Do NOT edit files under /opt/galion/galion-light,
   /opt/galion/bin, /opt/galion/console-ide, /opt/galion/hermes or /etc/systemd. You may READ them and COPY sources out.
   Never `systemctl enable/start` anything. Never write to Hermes state.db.
7. **Ports for tests:** only spare loopback ports 18091, 18002, 18646, 14020 (new builds). Kill your test processes when done.
8. **No secrets** in files, logs or replies. Never cat .env / galion-secrets.
9. **No provider/config changes** to Hermes (no OpenRouter key/config edits).
10. Toolchain: `export PATH=/root/.cargo/bin:$PATH` (cargo 1.99 is installed for root, not on PATH by default). git and rg are installed.

## 4. How you work (you are the LEAD)
- You are the lead. You do NOT write all the code yourself. Use `delegate_task` to spawn **at most 3** subagents in parallel:
  - **S1 (scaffold):** T1 then T2.
  - **S2 (ports):** T3 (needs T1 workspace; it may start by copying sources into crates/ while S1 finishes).
  - **S3 (de-python):** T4.
  - After T3 passes: T5 and T6 (one subagent, or yourself; T6 is mostly a `vite build` into a staging dir).
- Give each subagent: the ticket text, the HARD RULES, the CHECKS it must pass, and the paths. Start each brief with `DESK-ID: openmind.<ticket>`.
- A worker does not grade its own work: when a subagent reports done, YOU run CHECKS.md for that ticket and grade with the rubric.
- Write progress to /opt/galion/openmind/TEACHER/PROGRESS.md (append-only, newest at bottom, Zagreb timestamps):
  `- 2026-10-18T19:20:00+02:00 | T1 | S1 | PASS 9/10 | cargo test ok, fmt ok, clippy ok | next: T2`
- [SUPERSEDED 2026-10-28 by D-391-2: no git writes in the live tree; commit in your own clone] Commit after each passed ticket: `git -C /opt/galion/openmind commit -m "T1: ... (2026-10-18T19:20:00+02:00)"`. <!-- repo-check: allow (historical lesson text) -->
- If a check fails: send the subagent back with the exact failing output (max 2 retries), then record FAIL and move on.
- The teacher (Grok Bot) will review and may send corrections into this same session. Corrections override your plan.

## 5. Definition of done (whole foundation)
- `/opt/galion/openmind` is a git repo; `cargo build --release` of the whole workspace succeeds with `nice -n 19 ... -j 2`.
- LICENSE (MIT, "Copyright (c) 3000 Galion / galion-studio") at repo root and in every crate dir; `license = "MIT"` in every Cargo.toml.
- Crates: `openmind-schema`, `zagreb-time`, `galion-one`, `galion-desk`, `room`, `desk-hermes`, `link-watch` (names may be `openmind-*` prefixed; document the choice).
- All tickets T1–T6 have PASS lines in PROGRESS.md with CHECKS output summarised.
- `rg -l -i python crates/` returns nothing (except documented, comment-only references to the replaced script names, listed in PROGRESS.md).
- Nothing in production was changed. Staged items (T5 unit, T6 dist + switch/rollback scripts) are listed in TEACHER/STAGED.md as "awaiting approval".
