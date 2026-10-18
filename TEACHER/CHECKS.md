# CHECKS — self-evaluation programs (run by the LEAD, not by the worker who wrote the code)

Setup for every check: `cd /opt/galion/openmind && export PATH=/root/.cargo/bin:$PATH`
Paste a 1–3 line summary of each check result into PROGRESS.md. A ticket PASSES only if all its listed checks pass.

| ID | Check | Command | Pass when |
|---|---|---|---|
| C1 | format | `cargo fmt --all -- --check` | exit 0 |
| C2 | lint | `nice -n 19 cargo clippy -j 2 --workspace --all-targets -- -D warnings` | exit 0 |
| C3 | tests + release build | `nice -n 19 cargo test -j 2 --workspace` and `nice -n 19 cargo build -j 2 --release --workspace` | exit 0 both |
| C4 | binary size | `ls -l target/release/{galion-one,galion-desk,room,desk-hermes,link-watch}`; `file target/release/<bin>` | every bin < 2 MB and `file` says "stripped" |
| C5 | behaviour diff | start new binary on spare port (e.g. `ONE_BIND=127.0.0.1:18091 target/release/galion-one &`), then for each READ-ONLY route R: `diff <(curl -s -i 127.0.0.1:8091$R \| sed '/^Date:/d') <(curl -s -i 127.0.0.1:18091$R \| sed '/^Date:/d')`; kill the test process | diffs empty, or every difference explained (live data changed between calls) |
| C5b | golden output (T4) | `diff <(jq -S . old.json) <(jq -S . new.json)` | empty except documented timestamp fields |
| C6 | Zagreb time | `cargo test -p zagreb-time` and `rg -n 'Utc::now\(\)\|SystemTime::now\(\)' crates/ \| rg -v zagreb-time` lists only places that feed zagreb-time | tests pass; DST test 2026-10-25 present; no new output that prints UTC/Z |
| C7 | CJK scan | `rg -n '[\p{Han}\p{Hiragana}\p{Katakana}\p{Hangul}]' . --glob '!target' --glob '!stage/console-src/**'` | no matches (or only pre-existing in copied console-src, listed) |
| C8 | no Python | `rg -l -i 'python' crates/ deploy/` and `find . -name '*.py' -not -path './target/*' -not -path './stage/*'` | no .py files; rg hits only comments that name the replaced script, listed in PROGRESS.md |
| C9 | production untouched | `systemctl is-active hermes console galion-desk link-watch`; `curl -s -o /dev/null -w '%{http_code}' 127.0.0.1:8091/health`; `pgrep -f 'cargo\|target/release' ` after tests | all active, 8091 = 200, no leftover test processes |
| C10 | CPU-light | builds were run with `nice -n 19` and `-j 2`; max 3 subagents alive | true (state it in PROGRESS.md) |
| C11 | staged only (T5/T6) | `systemctl is-enabled galion-one console-prod 2>&1`; `bash -n deploy/*.sh` | units not installed/enabled; scripts parse; dry-run output pasted |

## Evaluation rubric (lead grades each subagent, 0–10; PASS needs ≥ 7 AND no hard-rule violation)
| Points | Criterion |
|---|---|
| 0–3 | **Correctness:** all listed CHECKS pass on first lead run (3), after 1 retry (2), after 2 retries (1). |
| 0–2 | **Fidelity:** behaviour identical to old (C5/C5b clean or every diff explained). |
| 0–2 | **Rules:** Zagreb times everywhere, no Python, no CJK, CPU-light, production untouched (any violation = automatic FAIL, score 0). |
| 0–2 | **Evidence:** report includes the exact commands run and their real output (no "should work"); sizes and diffs recorded. |
| 0–1 | **Scope discipline:** no unrequested refactors, no new heavy deps (no tokio/axum/hyper in T1–T4). |

Grade line format in PROGRESS.md:
`- <zagreb-ts> | T<n> | S<k> | PASS|FAIL <score>/10 | C1 ok C2 ok C3 ok ... | notes`
