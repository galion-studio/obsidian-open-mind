# CHECKS — run by the LEAD (Hermes) on every ticket; paste 1–3 line results into PROGRESS.md

Setup: `cd /opt/galion/openmind/release`. All commands are read-only. Use `nice -n 19`.

| ID | Check | Command | Pass when |
|---|---|---|---|
| K1 | claims traced | every claim in repo/README.md, site/index.html, marketing/* appears in FACTS.md / FACT-CHECK.md with `file:line` source or is marked `TODO(verify)`; spot-check 5 sources with `sed -n '<line>p' <file>` | 100% traced or TODO; spot-checks match |
| K2 | no secrets | `rg -n -i 'sk-[a-z0-9]{8,}\|BEGIN [A-Z ]*PRIVATE\|API_KEY=[^<_ ]\|ghp_\|xox[bp]-\|51\.38\.\|\bip50\b' .` | no matches |
| K3 | no Python | `find . -name '*.py'`; `rg -n 'python3? -c\|setup-python' .` | no files; rg hits only in CI "no-python" check text |
| K4 | MIT license | `test -f repo/LICENSE && head -1 repo/LICENSE` | "MIT License" and 2026 copyright line |
| K5 | Zagreb times | `rg -n '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}(Z\|\+00:00)' .`; every written timestamp has `+02:00`/`+01:00` or `CEST`/`CET` | no UTC/Z stamps |
| K6 | CJK scan | `rg -n '[\p{Han}\p{Hiragana}\p{Katakana}\p{Hangul}]' .` | no matches |
| K7 | forbidden content | `rg -n -i 'revolut\|guaranteed\|profit\|returns' repo site marketing brand` | no matches (or only "no profit promises" rule text in BRAND.md) |
| K8 | CI sanity | `rg -n 'cargo fmt\|cargo clippy\|cargo test\|\.py' repo/.github/workflows/ci.yml` | fmt+clippy+test+no-python steps present; YAML parses (`yq` if present, else careful read) |
| K9 | page weight | `du -cb site/* \| tail -1` and `rg -n 'https?://' site/` | total < 204800 bytes; no external URLs except `#TODO` placeholders |
| K10 | no frameworks | `rg -n -i 'react\|vue\|tailwind\|bootstrap\|jquery\|cdn\|<script src' site/` | no matches |
| K11 | write scope | `cd /opt/galion/openmind && git status --porcelain \| rg -v '^\?\? (release\|contrib)/'` shows nothing caused by you; nothing pushed, no remote added (`git remote -v` empty) | true |
| K12 | CPU-light | ≤ 2 subagents were alive at any time; heavy commands under nice | stated in PROGRESS.md |

## Rubric (teacher grades each ticket 0–3 per row)
| Criterion | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| Correctness / facts | invented claims | some untraced | all traced, minor gaps | all traced, TODOs explicit |
| Completeness | files missing | half | all files, thin | all files, useful content |
| Rules (K2–K7, K9–K11) | any hard-rule break | – | – | all pass |
| Clarity / tone | hype, unclear | wordy | clear | clear, short, plain |
| Lightweight | heavy deps/pages | – | under limits | well under limits |
Ticket PASSES at ≥ 11/15 with Rules = 3.
