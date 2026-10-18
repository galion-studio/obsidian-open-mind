# LESSON — Obsidian Open Mind open-source release (teacher: Grok Bot, lead: Hermes)

Written 2026-10-18T19:01:49+02:00 (Europe/Zagreb, CEST). Session: `openmind-release`. Work dir: `/opt/galion/openmind/release/`.
Base plan: `/opt/galion/openmind/contrib/grokbot/docs/main.md` (master plan) and `contrib/grokbot/docs/study/A-foundation.md` (facts).

## 1. What we are releasing
**Obsidian Open Mind** ("Open Mind") is the stance + code of the Obsidian stack by Galion Studio:
a lightweight, CPU-light, Rust-only foundation where the CPU box owns the loop, memory and keys, and any
GPU (NVIDIA / AMD / Apple, e.g. via exo or llama.cpp) is "just another Provider row". License: MIT (main.md §4).
The code is the Cargo workspace at `/opt/galion/openmind` that ANOTHER worker is building right now
(`crates/`, `Cargo.toml`, `TEACHER/`). Its state changes during your session: re-read it before you write a claim.
Milestone (main.md §9): **2026-10-31** — published, everything live, fact-checked, timestamped.
Your job is the release KIT (docs, site, branding, marketing drafts). Publishing is NOT your job.

## 2. HARD RULES (breaking one = ticket fails)
1. **Write only under `/opt/galion/openmind/release/`.** Read anything in `/opt/galion/openmind`, but NEVER edit
   `crates/`, `TEACHER/`, `Cargo.toml`, `Cargo.lock`, `contrib/`, and NEVER run `git add/commit/reset/checkout` in that repo.
   Do not touch any production service or file outside release/.
2. **Nothing leaves the box.** Do NOT create a GitHub repo, `git push`, publish, deploy the site, post to X/social, or email.
   Those need Slavik's approval: list them in `release/TEACHER/AWAITING-APPROVAL.md` instead.
3. **No Python.** Never write, run or add Python (no `python3 -c` either; use `jq`, `rg`, bash, Rust).
   The CI workflow you write must also FAIL if a `.py` file appears.
4. **Revolut / real money: NOT approved.** Never implement it, never advertise it, never list it as a feature or a TODO.
   Do not promise profit or returns in any marketing text.
5. **Time:** every timestamp you write is Europe/Zagreb with explicit offset, e.g. `2026-10-18T19:05:00+02:00`
   (CEST) / `+01:00` (CET from 2026-10-25 03:00 local). The VPS clock is UTC — never write server time or `Z`.
   Shell: `TZ=Europe/Zagreb date -Iseconds`.
6. **No stray CJK characters** (Chinese/Japanese/Korean) in anything you write. Scan and strip (CHECKS K6).
7. **CPU-light.** At most **2 subagents** alive at once (another swarm is already running). Prefix heavy commands with
   `nice -n 19`; cargo with `-j 1`. No polling faster than 60 s, no `watch`, no busy loops. Do not run `cargo build`
   yourself unless a check needs it; the other worker owns builds.
8. **No secrets.** Never read, print or copy contents of `/root/galion-secrets`, `.env` files, keys or tokens.
   No real IPs/hostnames of the VPS in release files (use placeholders).
9. **Facts only.** Every claim (feature, number, size, license, hardware support) must be traced to a file/line in
   `/opt/galion/openmind` or `contrib/grokbot/docs`, or marked `TODO(verify)`. Write the trace in `release/FACTS.md`.
   Not implemented yet = say "planned" + TODO, never present tense.
10. **Light front end.** Plain HTML + CSS (+ at most a few lines of vanilla JS). No frameworks, no CDN, no web fonts,
    no trackers, no external requests. Total page weight < 200 KB.

## 3. Team (you lead; max 2 subagents)
- **Hermes (lead):** owns R1 (repo prep) and final review; merges FACTS.md; fills PROGRESS.md; runs CHECKS.
- **Subagent A "site-brand":** R3 branding first, then R2 site (the site uses the branding).
- **Subagent B "marketing":** R4 marketing + the fact-check list; starts after R1 README draft exists (it sources from it).
A worker never grades its own ticket: the lead runs CHECKS on subagent output.

## 4. Deliverables layout
```
release/
  TEACHER/        LESSON.md TICKETS.md CHECKS.md (teacher, read-only for you) PROGRESS.md AWAITING-APPROVAL.md (you append)
  repo/           R1: README.md LICENSE CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md RELEASE-CHECKLIST.md
                  .github/ISSUE_TEMPLATE/{bug_report.md,feature_request.md} .github/pull_request_template.md
                  .github/workflows/ci.yml
  site/           R2: index.html style.css logo.svg (favicon optional)
  brand/          R3: BRAND.md logo.svg palette.md (or inside BRAND.md)
  marketing/      R4: announcement.md posts.md FACT-CHECK.md
  FACTS.md        claim -> source file:line (or TODO(verify))
```

## 5. How to report
Append to `release/TEACHER/PROGRESS.md` (newest at bottom): `<Zagreb RFC3339> | <ticket> | <who> | <what changed / check result>`.
When a ticket is done, say "R<n> READY FOR REVIEW". The teacher will review and send corrections in this session.
