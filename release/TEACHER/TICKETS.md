# TICKETS — open-source release kit (R1–R4)

Rules: LESSON.md §2. Checks: CHECKS.md. All output under /opt/galion/openmind/release/. Times: Europe/Zagreb with offset.
Order: R1 README draft first (lead) → R3 (A) and R4 (B) in parallel → R2 (A) after R3 → lead runs CHECKS on all.

## R1 — GitHub repo prep (lead: Hermes) → `release/repo/`
- `README.md`: what Obsidian Open Mind is (2–3 sentences); why CPU-light Rust foundation (CPU owns loop/memory/keys,
  GPUs are Provider rows); what is in the workspace TODAY (list crates that actually exist in `/opt/galion/openmind/crates/`
  at the time you write it, each with one line; anything not there yet = "planned"); MIT; quick start (`git clone <TODO url>`,
  `nice -n 19 cargo build -j 2 --release`, `cargo test`), requirements (Rust toolchain version from the repo, if pinned; else TODO);
  "Status" section with the 2026-10-31 milestone; timestamps format rule; links to CONTRIBUTING/SECURITY.
- `LICENSE`: MIT text, `Copyright (c) 2026 Slavik / Galion Studio and Obsidian Open Mind contributors`
  (must match the workspace root LICENSE holder once it exists; note in FACTS.md).
- `CONTRIBUTING.md`: build/test with nice/-j 2, `cargo fmt`/`clippy -D warnings`, no Python, Zagreb timestamps, DCO or simple sign-off (pick one, say which), small PRs.
- `CODE_OF_CONDUCT.md`: Contributor Covenant 2.1 (reference + contact placeholder `__CONTACT_EMAIL__`).
- `SECURITY.md`: how to report privately (placeholder contact), supported versions (0.1.x), no secrets in issues, key handling rules.
- `.github/ISSUE_TEMPLATE/bug_report.md`, `feature_request.md`, `.github/pull_request_template.md` (checklist: fmt, clippy, test, no Python, no secrets, Zagreb times, CJK scan).
- `.github/workflows/ci.yml`: on push/PR; jobs: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`; a `no-python` step that fails if `git ls-files '*.py'` is non-empty; a `cjk-scan` step (`grep -rP` for Han/Hiragana/Katakana/Hangul) ; a `secrets-scan` step (grep for `sk-`, `BEGIN .* PRIVATE`, `API_KEY=` with a value). No Python-based actions (no `actions/setup-python`).
- `RELEASE-CHECKLIST.md` toward **2026-10-31**: every item with owner + status + Zagreb timestamp; includes "all claims fact-checked (FACTS.md 100% traced or removed)", "LICENSE in root and every crate", "CI green", "secrets scan clean", "site weight < 200 KB", "Slavik approval for: repo creation, push, site publish, announcement posts".
- Done when: K1–K8 pass for repo/.

## R2 — Front end: static landing page (subagent A, after R3) → `release/site/`
- `index.html` + `style.css` (+ `logo.svg` from R3). Sections: hero (name + tagline from R3), what it is, why CPU-light Rust,
  what's in the box (only facts from FACTS.md), quick start (same as README), license (MIT), status + milestone date, footer with
  "last updated <Zagreb timestamp>". Links to GitHub = `#TODO-repo-url` placeholder.
- Semantic HTML, works without JS, responsive (one media query is enough), accessible (alt text, contrast AA, lang="en").
- No frameworks, no CDN, no external fonts/images/scripts, no analytics. Total weight < 200 KB (K9).
- Done when: K2, K3, K5, K6, K7, K9, K10 pass.

## R3 — Branding (subagent A) → `release/brand/`
- `BRAND.md`: name usage rules ("Obsidian Open Mind" full; "Open Mind" short; relation to Obsidian stack / Galion Studio;
  NOT affiliated with the Obsidian notes app — say so), 5 tagline options (≤ 8 words each, no unverifiable claims), voice & tone guide
  (plain, factual, no hype, no profit promises, numbers only if traced), do/don't examples.
- Color palette: 5–6 colors with hex, role (bg, fg, accent…), and WCAG contrast ratio for text pairs.
- `logo.svg`: simple hand-written SVG concept (< 5 KB, no embedded raster, no fonts required — text as optional), works on dark and light.
- Done when: K2, K3, K5, K6 pass, logo.svg < 5 KB and valid XML (`xmllint --noout` if available, else note).

## R4 — Marketing drafts (subagent B, after R1 README draft) → `release/marketing/`
- `announcement.md`: launch announcement draft (≤ 400 words) for 2026-10-31; every factual sentence has a footnote id that maps to FACTS.md.
- `posts.md`: 5 short posts (≤ 280 chars each, count shown), 1 longer thread (≤ 5 parts). Marked DRAFT — NOT POSTED.
- `FACT-CHECK.md`: table `claim | where used (file) | source (file:line in repo or contrib docs) | status (verified / TODO(verify) / removed)`.
  Any claim without a source must be removed or marked TODO(verify). No profit/returns claims; no Revolut/real-money mention.
- Done when: K1, K2, K3, K5, K6, K7 pass and FACT-CHECK has 0 unverified claims left in non-TODO text.
