# PROGRESS — release kit

Append-only. Newest at bottom. Times: Europe/Zagreb RFC 3339.

- 2026-10-18T19:01:49+02:00 | setup | teacher | teacher pack written (LESSON, TICKETS, CHECKS)
- 2026-10-18T19:06:00+02:00 | R1 | Hermes (lead) | repo/ written: README, LICENSE (MIT), CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, RELEASE-CHECKLIST, .github/workflows/ci.yml, 2 issue templates, PR template; FACTS.md opened
- 2026-10-18T19:09:10+02:00 | R2/R3 | subagent A "site-brand" | site/ + brand/ written (index.html, style.css, logo.svg; BRAND.md, CLAIMS.md)
- 2026-10-18T19:13:21+02:00 | R4 | subagent B "marketing" | marketing/ written (announcement.md, posts.md, FACT-CHECK.md)
- 2026-10-18T19:22:00+02:00 | lead | Hermes | teacher review #1 corrections applied, all 8 items

## Teacher review #1 corrections (2026-10-18T19:14:44 - 19:22:00+02:00)

1. "Rust-only" removed from repo/README.md, site/index.html, marketing/announcement.md and
   marketing/posts.md. It is not true today: `rg -n -i 'python' crates/` returns
   `crates/galion-one/src/main.rs:1136,1238,1830` and `crates/galion-desk/src/main.rs:236`,
   each a real spawn (`Command::new("python3")`). `find crates -name '*.py'` returns nothing.
   New wording: "Rust foundation ... TODO(verify)". The phrase comes from the dictated plan
   (`main.md:9`), not from the code. Every wording now adds, once, that the Hermes agent
   (upstream, Python) and the Node services are not part of the foundation (`A` section 1).
   brand/BRAND.md never contained "Rust-only" (checked with rg).
2. Present-tense design claims reworded. "Accelerators from NVIDIA, AMD or Apple are added
   ... as provider rows over an OpenAI-compatible endpoint" was traced to `A:76`, a study
   paragraph. Now: "is designed so that ... can be added", marked design, not code. Same in
   README, site, announcement [^10] and post P5. "the binaries stay small" (announcement
   [^9]) was replaced with the measured sizes listed below.
3. Site "the Rust parts ... about 10 MB of RAM" now says it is a 2026-10-18 snapshot of the
   pre-workspace binaries (`A:26`), and is not a measurement of the workspace crates. Same
   qualification in BRAND.md. Source row added to FACTS.md.
4. Site crate table now has a third column, "Source of the description", one `file:line` per
   row (`crates/<crate>/Cargo.toml:6`, plus `crates/desk-hermes/src/main.rs:3-8`). The
   "operator's own ear, mind and mouth", "Reads the pod, never writes it", "read-only view"
   and "same keys in the same order" lines are all traced. A `link-watch` row was added
   (`Cargo.toml` only, empty `src/`, no binary). README "What is in the workspace today" was
   re-listed from `ls -1 crates/` at 2026-10-18T19:14:44+02:00: seven entries, with build
   state per crate from `ls -l target/release/` at 19:16:54+02:00.
5. README links fixed: `../crates/openmind-schema/src/lib.rs` -> `crates/openmind-schema/src/lib.rs`.
6. BRAND.md: "by Dynalist Inc." removed; the not-affiliated-with-the-notes-app sentence kept.
7. No-profit / no-real-money rule kept and listed under K7 below.
8. Everything written stays under `release/`. No git command was run in the repo except the
   read-only `git status`, `git log` and `git remote -v`. No push, no repo, no posting.

## Measured facts added by the corrections

- `ls -l target/release/` 2026-10-18T19:16:54+02:00: galion-one 615624 B, galion-desk
  402720 B, room 344304 B, desk-hermes 1427472 B, libopenmind_schema.rlib 240750 B,
  libzagreb_time.rlib 93654 B. `link-watch` had no binary.
- `ls -1 crates/` 2026-10-18T19:14:44+02:00: seven entries (desk-hermes, galion-desk,
  galion-one, link-watch, openmind-schema, room, zagreb-time).
- `rg -n -i 'nvidia|cuda|rocm|metal' crates/` 2026-10-18T19:18:14+02:00: two hits, both
  hardware probing, not a provider (`galion-one/src/main.rs:3219` runs `nvidia-smi`; a mock
  fixture at `:1447` says `"device":"cuda"`). No accelerator `Provider` row exists.
- openmind-schema line numbers re-read 2026-10-18T19:17:58+02:00 (the file grew this session):
  Machine 17, ProviderKind 42-47, Provider 51-58, KeySlot 62-66, Surface 70-75, Stack 78-83,
  cpu_desk 86-132, stance strings 89-91, test 139-152. Stale traces to `:82-84`, `:50-58`,
  `:55-59`, `:133-145` were corrected in site/, brand/ and marketing/.
- Post character counts re-measured 2026-10-18T19:18:14+02:00 with `wc -m`: 261, 166, 163,
  144, 209; thread 158, 162, 139, 157, 184. All under 280.

## CHECKS K1-K12 - run by Hermes (lead) 2026-10-18T19:20:21 - 19:20:35+02:00

All commands read-only, `nice -n 19`. Run from `/opt/galion/openmind/release`.

- K1 claims traced — **PASS**. Every claim in repo/README.md, site/index.html and marketing/*
  is in FACTS.md or marketing/FACT-CHECK.md with a `file:line`; open items are marked
  TODO(verify). Nine spot-checks read back at 19:20:29+02:00, all match: lib.rs:89-91
  (`studio: "Galion Studio"`, `stack: "Obsidian"`, `stance: "Open Mind"`), lib.rs:4 ("A
  provider is an adapter, not a secret"), openmind-schema/Cargo.toml:6 (the description the
  site quotes), Cargo.toml:24-28 (`opt-level="z"`, `lto`, `codegen-units=1`, `panic="abort"`,
  `strip`), A-foundation.md:26 ("about 10 MB of RAM"), galion-one/src/main.rs:1136
  (`Command::new("python3")`), galion-desk/src/main.rs:236 (`Command::new("python3")`),
  main.md:9 (the plan's "Rust-only" phrase), desk-hermes/src/main.rs:3-8 (read-only database,
  same keys in the same order). 9 of 9.
- K2 no secrets — **PASS**. Two hits, both the check text itself
  (`TEACHER/TICKETS.md:19`, `TEACHER/CHECKS.md:8`). No credential, no private key, no host
  address, no `ip50` in any release file.
- K3 no Python — **PASS**. `find . -name '*.py'` returns nothing.
  `rg -n 'python3? -c|setup-python'` hits only prose: `FACTS.md:46` (describing the defect),
  `TEACHER/LESSON.md:21`, `TEACHER/TICKETS.md:19`, `TEACHER/CHECKS.md:9` (rule text), plus the
  README sentence that says the build needs no Python. No Python file, no Python invocation.
- K4 MIT license — **PASS**. `repo/LICENSE:1` is `MIT License`; `:3` is
  `Copyright (c) 2026 Slavik / Galion Studio and Obsidian Open Mind contributors`.
- K5 Zagreb times — **PASS**. `rg` for `YYYY-MM-DDTHH:MM:SSZ` or `+00:00` returns no matches.
  Every written timestamp carries `+02:00` or `CEST`.
- K6 CJK scan — **PASS**. No Han, Hiragana, Katakana or Hangul character anywhere in release/.
- K7 forbidden content — **PASS**, and the five hits are not content claims:
  `brand/BRAND.md:76` (the "No promises about money ... no real-money rails" rule text),
  `repo/RELEASE-CHECKLIST.md:68` (the "K7 clean" checklist line), `repo/README.md:10` and
  `:47` and `marketing/announcement.md:45` (the ordinary English word "returns" in "returns
  nothing" and "returns seven crates"). No Revolut, no guaranteed return, no earnings claim.
- K8 CI sanity — **PASS**. `repo/.github/workflows/ci.yml` has `cargo fmt --all -- --check`
  (:24), `cargo clippy --workspace --all-targets -- -D warnings` (:35),
  `cargo test --workspace` (:44), a no-python step over `git ls-files '*.py'` (:54-59), a cjk
  step and a secrets step.
- K9 page weight — **PASS**. `du -cb site/*` = 15486 bytes total, well under 204800. The only
  `http` string is the SVG namespace `http://www.w3.org/2000/svg` in `site/logo.svg:2`, an XML
  namespace, not a request. The only link is `#TODO-repo-url`, an in-page placeholder.
- K10 no frameworks — **PASS**. No react, vue, tailwind, bootstrap, jquery, cdn or
  `<script src` in site/. One local stylesheet and one local SVG.
- K11 write scope — **PASS**. `git status --porcelain` filtered of `release/` and `contrib/`
  shows ` M Cargo.lock` and untracked `crates/*` and `docs/` — the other worker's build, none
  of it mine. `git remote -v` is empty. Nothing pushed. No repo created. No git write command
  was used anywhere in this turn.
- K12 CPU-light — **PASS**. Exactly two subagents were ever alive (A "site-brand" R3/R2,
  B "marketing" R4), both finished before this battery. Heavy commands ran under `nice -n 19`;
  the lead started no cargo build, no test run and no benchmark.

## Status

- R1 READY FOR REVIEW (2026-10-18T19:22:00+02:00)
- R2 READY FOR REVIEW (2026-10-18T19:22:00+02:00)
- R3 READY FOR REVIEW (2026-10-18T19:22:00+02:00)
- R4 READY FOR REVIEW (2026-10-18T19:22:00+02:00)

Left as TODO(verify), all listed in FACTS.md: the Python-free Rust binaries; an accelerator
`Provider` row (still design only); the `link-watch` build; `openmind-http` and
`openmind-state` (not in the tree); the minimum supported Rust version; the public repository
URL; the 2026-10-31 milestone itself.

## Teacher review #2 — tone/placement round, run by Hermes (lead) 2026-10-18T19:25 - 19:27+02:00

Applied: intro/hero cleaned and Python caveat moved to "Status / known gaps" (repo/README.md,
site/index.html); "Status" section and nav link added to site/index.html; one "Status"
paragraph added to marketing/announcement.md and its body de-numbered; line numbers removed
from the README crate table, the site crate table and the site "why" list (files named only);
FACTS.md gained a review-#2 section recording the rule "user-facing prose cites files, not
line numbers". Files changed: repo/README.md, site/index.html, marketing/announcement.md,
FACTS.md, TEACHER/PROGRESS.md. No new subagents.

- K1 claims traced — **PASS** (re-run 2026-10-18T19:26:26+02:00). Read-backs after the edit:
  lib.rs:3 (processor owns the loop, skills, memory, key box), lib.rs:4 (provider is an
  adapter; video memory is only for a model), lib.rs:42 (`pub enum ProviderKind`), lib.rs:49
  (a new row, not a new program), lib.rs:89-91 (`studio: "Galion Studio"`, `stack:
  "Obsidian"`, `stance: "Open Mind"`), desk-hermes/Cargo.toml:6 (the quoted description),
  A-foundation.md:76 (the accelerator design rule, now cited without a line number in prose).
  7 of 7 match; every Status item resolves to a FACTS.md row group.
- K6 CJK scan — **PASS** (re-run 2026-10-18T19:26:31+02:00). Han, Hiragana, Katakana and
  Hangul scan over release/ returns 0 matches.
- K7 forbidden content — **PASS** (re-run 2026-10-18T19:26:31+02:00). Four hits, all rule or
  check text, no claim: brand/BRAND.md:76 (the "no promises about money" rule),
  repo/CONTRIBUTING.md:60 (the no-money-rails rule), marketing/FACT-CHECK.md:7 and :54 (the
  out-of-scope note and the checklist row). No Revolut, no earnings claim, no guaranteed
  return.
- K9 page weight — **PASS** (re-run 2026-10-18T19:26:31+02:00). `du -cb site/*` = 16696 bytes,
  under 204800. The only `http` string is the SVG namespace in site/logo.svg:2. Every href/src
  in index.html is local or in-page (logo.svg, style.css, #top, #what, #why, #box, #start,
  #status, #TODO-repo-url); no external request.

RELEASE KIT REVIEWED - awaiting Slavik approval for repo/push/publish/posts
