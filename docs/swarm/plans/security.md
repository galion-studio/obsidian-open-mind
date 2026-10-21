<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Sector plan: security (fuzzing, SAST, SEC-1..5, launch gates, HADES, sandbox, licences, public export)

Written 2026-10-20 ~20:50 CEST (+02:00), planning only (no code, no commits, no builds, no VPS access; no secret contents read;
secret-holding files are named by path only, as the source docs already do). Launch deadline 2026-10-31.
Note: CEST ends 2026-10-25 03:00; from then on times are CET +01:00.

Sources read: /workspace/swarm/{PLANNER-BRIEF,RULES,WAVE2,CALL-RECORD,CALL-RECAP}.md; /workspace/review-20261020/{CODE,LIVE,PLAN-DIGEST,PROGRESS,REVIEW,SESSION}.md;
wt/integration-1 (8fd26f6) docs/launch/{GATES-OCT31,PUBLIC-EXPORT,SCORECARD-READINESS}.md, docs/openssf.md, docs/hades-gate.md,
docs/real-tests/{hades-approval,sandbox,edge-boot,ucpu,hermes-tools,launch-gates,integration-1}.md, docs/third-party/{ucpu,room}.md,
docs/integration/integration-1.md, TEACHER/BACKLOG.md; in-progress worktrees wt/fuzz-sast and wt/sec-runbooks (uncommitted files, listed below).

## 1. Sector scope

| Area | What it covers | Owner-only parts |
|---|---|---|
| Fuzzing | cargo-fuzz/libFuzzer targets for every parser that sees untrusted bytes; corpus; CI smoke; Scorecard "Fuzzing" | none (CI minutes on GitHub are free) |
| SAST | Scorecard "SAST" credit (CodeQL), clippy SARIF, unsafe/SAFETY audit, Miri where possible | enabling code scanning in GitHub settings; CodeQL licence ruling |
| SEC-1..5 | runbooks + tooling for: SEC-1 key rotation (incl. the GitHub token in `galion-light/secrets/persist` that INFRASTRUCTURE.md calls leaked) + history purge without Python tools; SEC-2 log containing a token; SEC-3 Hermes `config.yaml`/`SOUL.md` 666, `vault/` 777; SEC-4 private key copy in the from-grok-build snapshot; SEC-5 tuner-quick trycloudflare tunnel | **every execution step** (rotation, chmod, delete, tunnel stop, DNS) is Slavik's (GATES-OCT31.md:29-33, BACKLOG.md:25-28,99-100) |
| Launch gates | OpenSSF Scorecard readiness, REUSE 3.3, SBOM (CycloneDX), gitleaks, cargo-deny, `scripts/public-export.sh` 10 gates | GitHub settings, contact address, visibility/new public repo (LAUNCH-1/2) |
| HADES admin safety | hades-gate Ed25519 single-use tokens, admin-safety deny rule, disable/stop, key custody | admin key generation and custody; any apply on prod |
| Seccomp / sandbox | openmind-sandbox (userns, Landlock, caps, per-run cgroup), omnode seccomp deny-list (x86_64, aarch64, armv7) | a privileged test host; real aarch64 hardware |
| Licence question | GPL/LGPL **test-only tools** (strace LGPL-2.1+, util-linux GPL-2.0+, qemu-user GPL-2.0), kernel syscall-number constants (GPL-2.0 WITH Linux-syscall-note), plus new ones found here: libFuzzer's NCSA licence, CodeQL CLI terms, Semgrep CE (LGPL-2.1) | ruling |
| Public IP literals | gate 10 WARN: 20 public IPv4 literals + 1 `*.trycloudflare.com` hostname in the exported tree | keep / redact / exclude decision |

Out of scope here (other sectors): live cutovers, nerve/cortex, Gaia function wiring, VPS ops beyond the SEC owner steps.

## 2. Current state (2026-10-20 20:42 +02:00)

| Item | Branch / SHA | State | Evidence |
|---|---|---|---|
| launch-gates | swarm/launch-gates **79ee761** (pushed, wave 1); merged in integration-1 8fd26f6 | REUSE.toml + LICENSES/, 13 `.license` companions; deny.toml = allow-list; 36 CycloneDX files / 130 components; gitleaks 8.30.1 over export = 0 and over 251 commits of history = 0; all `uses:` SHA-pinned; `public-export.sh` 10 gates; `.github/workflows/launch-gates.yml` | docs/launch/GATES-OCT31.md:11-24, docs/real-tests/launch-gates.md |
| public-export on integration-1 | 8fd26f6 (local, not pushed per CALL-RECAP) | 0 FAIL, 1 WARN: 20 public IPv4 + 1 quick-tunnel hostname (was 7 IPs at launch-gates' run) | docs/real-tests/integration-1.md:22, integration/integration-1.md "Needs" #4 |
| Gate-10 hits by file (my count at 8fd26f6, same filter as public-export.sh:291) | | docs/TERMINAL-LOG.md 3 (box egress IP on VPS fail2ban allow-list), crates/repo-check/tests/checks.rs 3 (fixtures), lan-discovery docs/script/src 7, openmind-nerve src 4, stack/hermes/SOUL.md 1 + TEACHER/QUEUE.md 1 (France server), docs/real-tests/survival-heal.md 1 (VPS IP), hermes-tools fixture 1; hostname in docs/session-2026-10-18-evening.md:45 | `git grep` at 8fd26f6 |
| Scorecard readiness | in launch-gates | Fuzzing **missing**, SAST **partial** (clippy only), Branch-Protection / Code-Review / Security-Policy (`__CONTACT_EMAIL__`) / Webhooks **owner**, Maintained/Contributors **cannot be met by Oct 31** | docs/launch/SCORECARD-READINESS.md:15-33 |
| fuzz-sast (slot 25, running) | swarm/fuzz-sast **bb89351** = merge of common 26d98b0 + lan-discovery fe5f0bd, no own commit yet | uncommitted: `fuzz/` (own workspace, `exclude = ["contrib","fuzz"]`), 4 targets: http_parse_head, base64_decode, json_escape (galion-common), mdns_parse (openmind-discovery); corpus 1787/92/995/724 files; one finding already (JSON float round-trip, fixed in the harness via serde_json `float_roundtrip`, comment refers to docs/fuzzing.md "finding 1", file not yet written); libfuzzer-sys 0.4.13 licence (MIT OR Apache-2.0) AND **NCSA** | wt/fuzz-sast/fuzz/Cargo.toml, `git status` |
| sec-runbooks (slot 26, running) | swarm/sec-runbooks at **13a296c**, no commit yet | uncommitted: `crates/history-purge` (`galion-purge`, std-only Rust, drives git fast-export/fast-import; refuses non-bare, shallow, `/opt/galion`, remotes; re-scans all trees after rewrite), docs/launch/{SEC-RUNBOOKS,SEC-1-ROTATION,SEC-1-HISTORY-PURGE}.md, purge-paths.obsidian-open-mind.txt, deploy/staged/sec3-hermes-perms.sh (dry run default, rollback file), scripts/real/sec-runbooks.sh; log /workspace/swarm/target/sec-runbooks-real.log | wt/sec-runbooks |
| SEC-1 | owner | 18-row inventory, P0: GitHub token (leaked), cloudflared tunnel credential + token that leaked into `gateway-deaths.log`, VPS root password seen in a 2026-10-18 screenshot | SEC-1-ROTATION.md:38-66 |
| SEC-2 | owner | log already 0600 since 2026-10-18 20:04, `secrets/persist` 0700 since 22:01; remaining exposure = root + 6-hourly backups + Hermes `sessions/` request dumps (2,009) | SEC-RUNBOOKS.md:30-66 |
| SEC-3 | owner | config.yaml 666, SOUL.md 666, vault/ 777, vault/.vault.lock 666 (parent dir 700) | SEC-RUNBOOKS.md:68-81, PROGRESS.md:31 |
| SEC-4 | owner | `/opt/galion/from-grok-build/workspace/.ssh/id_ed25519` (0600), `from-grok-build/root/galion-secrets/`, a 0755 secrets dir with an example file | SEC-RUNBOOKS.md:113-122 |
| SEC-5 | owner | tuner-quick trycloudflare still running; options A stop (recommended) / B named hostname / C supervised random URL | SEC-RUNBOOKS.md:138-152, docs/cutover/tuner.md:134 |
| hades-gate | swarm/hades-approval **ccaab42**, gaia-hades-tokens **f3f41a2** | Ed25519 `verify_strict`, single-use ledger, scope + expiry, `approved-test` dead, kill/rotate always refused, admin-safety checked before token, disable file + `/v1/stop`; tests lib 17, approval 12, decide 5, journal 5, simd_free 1, real 1; real script PASS incl. replay/tamper/forged/scope/expiry/`rm -rf`/`usermod` | docs/hades-gate.md:8-40, real-tests/hades-approval.md:20-118 |
| HADES admin-safety design | in ccaab42 | `safety.rs` (410 lines) is a **substring/token deny-list** over printable-ASCII plan text plus protected targets; opaque plans refused. Not yet adversarially tested against shell indirection | crates/hades-gate/src/safety.rs:236-290 |
| HADES wiring | | gaia heal-tick uses tokens on the box; nothing live; HADES scores 22 % | PROGRESS.md:22,171 |
| sandbox | swarm/sandbox **d194c19** (hermes-tools 8ccdfcc includes it) | EMFILE root cause fixed; escape 7/7 blocked; stress 24/25; privileged host test **not done**; docs-sync saw `group_kill_leaves_no_grandchild` fail once | real-tests/sandbox.md:85,185-212; docs-sync.md:73 |
| omnode seccomp | swarm/edge-boot **753676e** (needs ucpu **3e70ffb** first) | x86_64 deny-list enforced (strace/unshare EPERM); aarch64 under qemu fails closed (filter cannot load); **real aarch64 hardware enforcement unproven**; armv7 seccomp fix in ucpu | real-tests/edge-boot.md:36-44, ucpu third-party |
| unsafe hygiene | | openmind-sandbox 30 `unsafe` (all SAFETY per CODE.md:47); edge/openmind-node ~43 `unsafe` with ~12 SAFETY comments; VPS galion-one 10 raw FFI blocks without SAFETY | `grep -c` at 8fd26f6; CODE.md:47 |
| CI | integration-1 | workflows: launch-gates, realtest, release, sbom, scorecard (skips while private), self-eval (gitleaks, licence). No fuzz, no CodeQL | .github/workflows/ |
| Tools on box | | gitleaks/cargo-deny/cyclonedx/comply under /workspace/swarm/target/launch-gates/tools; nightly toolchain + cargo-miri installed; strace, qemu-aarch64-static present; 8 cores, 15 GB RAM, 68 GB free disk | `rustup toolchain list`, `nproc`, `df` |

## 3. Sessions

Agent counts: **target** = what the owner's "~50 per task" plan would put on it; **useful max** = how many agents can work it in
parallel without colliding (honest; most of this sector is serial or owner-gated). Ports: security slots 25/26 keep 19250-19259 /
19260-19269; new agents take free slots per RULES.md.

### S0. Finish the two running agents (now to ~22:00 Oct 20)
Goal: land slot 25 and slot 26 as pushed branches.
| Task | Target | Useful max | Parallel units | Inputs | Success criteria (real-life) |
|---|---|---|---|---|---|
| S0.1 fuzz-sast commit+push (4 targets, docs/fuzzing.md with finding 1, docs/third-party/fuzz-sast.md, corpus minimised) | 1 | 1 | none (one worktree) | bb89351 | each target runs `cargo +nightly fuzz run <t> -- -max_total_time=300` with 0 crashes, corpus replay `-runs=0` passes; finding 1 has a regression seed (`seeds/json_escape/regress-float-1ulp` exists) |
| S0.2 sec-runbooks commit+push | 1 | 1 | none | 13a296c + docs/launch from launch-gates | `GALION_REAL=1 scripts/real/sec-runbooks.sh` passes: galion-purge on a scratch `git clone --mirror` of base.git removes every path in purge-paths.txt, old blob ids gone (`cat-file --batch-check`), gitleaks 0 on the result, refusals proven (non-bare, remote present, `/opt/galion`); sec3 script dry run on a fake tree with 666/777 modes, apply + rollback restore exact modes |
Approval: none (branch pushes are allowed). Dependency: none.

### S1. Fuzzing breadth (Oct 21-23)
Goal: Scorecard "Fuzzing" credit and real coverage of every untrusted-input parser.
Parallel unit = **one fuzz target** (each a new `fuzz/fuzz_targets/<t>.rs` + seeds; the shared `fuzz/Cargo.toml` `[[bin]]` list is the only
merge conflict, solved by each agent adding its own block and a final merge agent).
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S1.1 new targets, one agent per target: hades-gate token parse/verify, hades-gate config TOML subset, hades-gate `safety::review_plan`, omnode `pair::verify` payload, openmind-config, openmind-schema, openmind-nerve logscan + replay, openmind-memory query/JSON body, hermes-tools path resolution (fs_read/fs_write jail), lan-discovery admit/probe parsing, tuner request routing, galion-one HTTP head + pairing body, link-watch config, openmind-llama/exo/tulu/speech fixture parsers (1 agent for the 4), room request parser, galion-common http body/chunked | 50 | **16** (one per target; >16 targets do not exist today) | per target | S0.1 merged; integration-1 8fd26f6 as base (has all crates) | each target: 30 min box run (nice, -j1, `-rss_limit_mb=1024`) 0 crashes **or** each crash fixed with a regression seed + unit test; crate tests still pass |
| S1.2 differential/invariant targets (round-trip encode/decode; hades token sign→verify→reject-on-any-bit-flip; base64/hex/json vs reference crates as dev-deps of fuzz only) | 10 | 4 | per codec | S1.1 | invariants hold over 1 h each |
| S1.3 CI: `fuzz.yml` (nightly, 60 s smoke per target on PR, 10 min per target on a weekly cron); optional ClusterFuzzLite (Apache-2.0) | 2 | 1 | none | S1.1 | workflow passes in the branch's CI run; Scorecard local check (below S5.3) reports Fuzzing detected |
| S1.4 merge agent: one `swarm/fuzz-all` merging every S1.1 branch | 1 | 1 | serial | S1.1-S1.3 | all targets build, corpus replay passes |
CPU reality: box has 8 cores shared by ~16-20 agents. Only **3-4 fuzz runs at once** on the box; long campaigns belong to CI (free on public repos only;
on the private repo Actions minutes are limited, so long runs wait until after LAUNCH-1). Approval: NCSA licence for libFuzzer (fuzz-only) in the licence ruling (S6).

### S2. SAST and unsafe audit (Oct 21-24)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S2.1 `codeql.yml` (github/codeql-action, `languages: rust`, SHA-pinned, `security-events: write` at job level only) | 2 | 1 | none | launch-gates workflows | workflow lints (actionlint, MIT) and runs on a PR; zero high alerts or each triaged in docs/sast.md. Runs only when code scanning is enabled (free for public repos) |
| S2.2 clippy SARIF upload (clippy-sarif + sarif-fmt, MIT) + a stricter lint set (`clippy::undocumented_unsafe_blocks`, `unwrap_used` in non-test code, `indexing_slicing` on parsers) per crate | 30 | **8** (per crate group: edge, sandbox/cgroup, hades, memory, nerve, one/desk/tuner, discovery/lan, common/config/schema) | per crate group | integration-1 | `cargo clippy --all-targets -- -D warnings` green with the new lints; every remaining allow has a reason comment |
| S2.3 SAFETY comments + review for edge/openmind-node (~43 unsafe, ~12 SAFETY) and openmind-sandbox | 4 | 2 (edge, sandbox) | per crate | edge-boot 753676e, sandbox d194c19 | `undocumented_unsafe_blocks` = 0; seccomp/sandbox real tests still pass on x86_64 and qemu |
| S2.4 Miri on the pure-logic crates (galion-common, hades-gate token/ledger in-memory, openmind-schema) | 3 | 3 | per crate | nightly + cargo-miri on box | `cargo +nightly miri test -p <crate>` clean (FFI/syscall tests skipped with `cfg(miri)`) |
| S2.5 `cargo deny`: set `publish = false` per crate, flip `wildcards` to `deny` | 1 | 1 | none | GATES-OCT31.md "Known gaps" | gate 8 passes with `deny` |
Approval: CodeQL CLI is under GitHub's CodeQL terms (not on the RULES.md allow-list; CI-hosted tool, not a dependency) -> S6 ruling; Semgrep CE (LGPL-2.1) is **not** proposed.
Not proposed: VPS galion-one 10 FFI blocks (VPS-only source, out of the GitHub tree).

### S3. HADES admin safety (Oct 21-25)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S3.1 adversarial review of `safety.rs` deny-list: shell indirection (`$(...)`, backticks, `eval`, `sh -c`, base64/hex-encoded payloads, `${IFS}`, aliases, `find -delete`, `dd of=/dev/`, `truncate`, `>` redirections onto config, `systemctl mask`, `crontab`, `ssh-keygen`, `chattr`), Unicode/homoglyph (already ASCII-only: verify), path traversal in targets | 10 | **3** (shell-syntax family, file-destruction family, access/credential family) | per bypass family | ccaab42 | a table of every attempted bypass with result; each bypass that gets `WouldApply` becomes a failing test, then a fix (likely: allow-list of plan verbs instead of deny-list) |
| S3.2 fuzz `review_plan` + token (shared with S1.1, not double-counted) | - | - | - | S1.1 | - |
| S3.3 key custody + clock: admin seed generation offline, where the public key lives, rotation and revocation of the admin key (ledger keeps old nonces), TTL behaviour across the 2026-10-25 03:00 CEST->CET change (tokens use unix time? prove it), clock-skew window | 3 | 2 | custody doc / clock tests | ccaab42, f3f41a2 | tests with faked clock across the DST instant; real script: revoked key refused, old token refused after key rotation |
| S3.4 ledger robustness: concurrent `check` of the same token from 2 processes (single-use under race), ledger on full disk / read-only fs -> fail closed | 3 | 2 | race / fs-failure | ccaab42 | real test: 50 parallel uses of one token -> exactly 1 `WouldApply`; read-only ledger -> all refused |
Approval: generating the real admin key pair and where it is stored (Slavik); wiring HADES to any prod apply path stays owner-gated.

### S4. Seccomp / sandbox evidence (Oct 22-26)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S4.1 sandbox flake: 200x stress of `group_kill_leaves_no_grandchild` and the 25x suite on integration-1 | 2 | 1 | none | d194c19, docs-sync.md:73 | >=199/200 with root cause named, or 200/200 after fix |
| S4.2 escape suite extension, one agent per family: ptrace/process_vm, mount/pivot_root, userns nesting, bpf/perf_event, io_uring, keyctl/add_key, socket families (AF_PACKET/NETLINK), fs escapes via `/proc/*/root` and fd passing | 20 | **4** | per syscall family | hermes-tools 8ccdfcc, edge-boot 753676e | each attempt blocked on x86_64 with the expected errno; recorded in docs/real-tests |
| S4.3 deny-list parity across x86_64 / aarch64 / armv7 (generated table from the syscall lists, checked in CI) | 2 | 1 | none | ucpu 3e70ffb + edge-boot | test fails if a syscall is denied on one arch and missing on another |
| S4.4 privileged/host test of openmind-sandbox (`net`, `j_net`, cgroup delegation) on a **test** host, never /opt/galion/openmind | 1 | 1 | none | sandbox.md:203-212 | `features()` and net tests pass on the host kernel; report |
| S4.5 real aarch64 seccomp enforcement (owner phone/board, or a free arm64 CI runner on the public repo) | 1 | 1 | none | edge-boot.md:43 | `sandbox selftest` reports `installed: true`; `strace -f true` EPERM on real arm64 |
Approval: S4.4 needs a test host that Slavik names (RULES.md forbids building/writing on the VPS); S4.5 needs an owner device or arm64 runner after LAUNCH-1.

### S5. Launch-gate consolidation and public-export cleanup (Oct 23-29)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S5.1 gate-10 cleanup proposals (doc/fixture only): move test fixtures to RFC 5737 TEST-NET ranges (lan-discovery, nerve, hermes-tools, repo-check), replace the VPS IP in survival-heal.md and TERMINAL-LOG.md with `<vps>`/`<box>`, propose export-ignore for TEACHER/, admin.preferences, stack/ (with the arena carve-out in PUBLIC-EXPORT.md) and **docs/launch/SEC-*.md** (sec-runbooks docs name secret paths and the tunnel id prefix) | 6 | **3** (fixtures in code / docs / export rules) | per file group | integration-1 + S0.2 | public-export gate 10: 0 IPv4 outside TEST-NET or each remaining one owner-accepted; quick-tunnel hostnames 0 |
| S5.2 re-run the full `public-export.sh` (all 10 gates + `--online` advisories) on every merge candidate: integration-1, gaia-land when pushed, fuzz-all, the final release candidate | 4 | 2 (independent re-runs, N-version) | per candidate ref | S1.4, S2, S5.1 | 0 FAIL, 0 WARN (or owner-accepted WARN); MANIFEST.sha256 identical between the two independent runs |
| S5.3 Scorecard local dry run: scorecard CLI (Apache-2.0) `--local` checks that work offline (Binary-Artifacts, Dangerous-Workflow, Pinned-Dependencies, Token-Permissions, SAST/Fuzzing detection) on the exported tree | 1 | 1 | none | S1.3, S2.1 | report in docs/launch/SCORECARD-READINESS.md with per-check score; no new accounts or publishing |
| S5.4 SBOM + REUSE + gitleaks consistency: one count table (fix the 723 vs 812 REUSE confusion, CALL-RECAP §4.13) | 1 | 1 | none | launch-gates | counts recorded with refs; comply and gitleaks reproduced |
| S5.5 post-purge verification: gitleaks + path-policy over the **purged mirror** produced by the owner (S7) and over the new single-root public tree | 1 | 1 | none | S7.2 | gitleaks 0, no purge-list path in any tree of any ref, commit map present |
Approval: export scope (stack/, TEACHER/, admin.preferences, SEC docs), accepting any remaining IP literal, `__CONTACT_EMAIL__`, all GitHub settings (branch protection, private vulnerability reporting, secret scanning + push protection, Dependabot alerts, code scanning, tag ruleset), LAUNCH-1 (new public repo from the exported tree, never flip the private one; PUBLIC-EXPORT.md "The rule").

### S6. Licence ruling pack (Oct 21, one sitting)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S6.1 one-page decision memo: (a) GPL/LGPL **test-only tools** executed as programs, never linked or shipped: strace (LGPL-2.1+), util-linux unshare/nsenter (GPL-2.0+), qemu-user-static (GPL-2.0), git itself (GPL-2.0, already used by galion-purge); (b) syscall numbers/ABI constants from kernel UAPI (GPL-2.0 WITH Linux-syscall-note, facts/interface exception); (c) libFuzzer NCSA (fuzz-only, never in a release); (d) CodeQL CLI terms (CI service, nothing vendored); (e) explicitly rejected: BFG (GPL-3.0, JVM), git-filter-repo (Python), Semgrep CE (LGPL-2.1). Proposed rule text for RULES.md: "allow-list applies to code linked into or shipped with our binaries; external tools run in tests/CI may be any OSI licence, are recorded in docs/third-party, and must never be vendored" | 1 | 1 | none | third-party/{ucpu,room}.md, S0.1 | Slavik answers yes/no per line; deny.toml unchanged (it already guards linked crates) |
Approval: the whole memo.

### S7. Owner execution of SEC-1..5 with swarm verification (Oct 21-30)
The swarm does **not** execute any of these (RULES.md 2-5). Agents only prepare, then verify read-only after Slavik acts.
| Task | Target | Useful max | Parallel units | Inputs | Success criteria (owner acts, swarm verifies) |
|---|---|---|---|---|---|
| S7.1 SEC-1 rotation, order P0 (Oct 21-22: GitHub token, cloudflared tunnel credential + token, VPS root password) -> P1 (Oct 22-26) -> P2 dormant (by Oct 29) | 1 | 1 | none (one verifier) | SEC-1-ROTATION.md | after each row: the named consumer still healthy (read-only GET of its health endpoint, `systemctl status`), old credential refused where checkable without printing it |
| S7.2 SEC-1 history purge with `galion-purge` on a scratch mirror (Oct 27-30), then the public repo is created from the exported tree | 1 | 1 | none | SEC-1-HISTORY-PURGE.md, S0.2 | S5.5 passes on the result |
| S7.3 SEC-2 move/scrub `gateway-deaths.log` after tunnel rotation; prune Hermes request dumps | 0 | 0 | | SEC-RUNBOOKS.md:30-66 | count-only grep returns 0 |
| S7.4 SEC-3 run `deploy/staged/sec3-hermes-perms.sh` (dry run, then apply) | 0 | 1 verifier | | SEC-RUNBOOKS.md:68-110 | `stat -c %a` shows 600/644/700/600; hermes-rs still answers its health GET |
| S7.5 SEC-4 delete or move the from-grok-build key copy and secret folders (after SEC-1 row 2 rotates that key) | 0 | 0 | | SEC-RUNBOOKS.md:113-122 | `test -e` false on the listed paths (no read) |
| S7.6 SEC-5 option A `systemctl stop tuner-quick` (or B/C) | 0 | 0 | | SEC-RUNBOOKS.md:138-152 | quick-tunnel hostname no longer resolves/answers; tuner 8650 still healthy on loopback |
| S7.7 push-path freeze: rotating the GitHub token (row 1) and later the box key (row 18) breaks `/workspace/swarm/push-branch.sh`; schedule a 30-min no-push window and re-test one push after | 1 | 1 | none | SEC-1-ROTATION.md rows 1, 18 | one test push of a swarm/* branch succeeds after rotation |
Approval: every row (rotations, chmod, deletes, tunnel stop, DNS for option B, history rewrite, new public repo).

### S8. Final security go/no-go (Oct 30)
| Task | Target | Useful max | Parallel units | Inputs | Success criteria |
|---|---|---|---|---|---|
| S8.1 independent final checklist run (RELEASE-CHECKLIST.md section 7, GATES-OCT31.md) by 2 agents on the release candidate; diff their reports | 2 | 2 | per independent reviewer | all above | both reports agree: gates 1-10 green, fuzz smoke green, CodeQL no open high, SEC-1..5 verified closed, contact filled; any disagreement goes to Slavik |
Approval: LAUNCH-2 sign-off.

## 4. Totals

| Session | Tasks | Target agents | Useful max agents |
|---|---|---|---|
| S0 finish running agents | 2 | 2 | 2 |
| S1 fuzzing breadth | 4 | 63 | 22 |
| S2 SAST + unsafe | 5 | 40 | 15 |
| S3 HADES admin safety | 3 (+1 shared with S1) | 16 | 7 |
| S4 seccomp/sandbox | 5 | 26 | 8 |
| S5 launch gates + export | 5 | 13 | 8 |
| S6 licence ruling | 1 | 1 | 1 |
| S7 SEC-1..5 (owner) + verification | 7 | 3 | 4 |
| S8 final go/no-go | 1 | 2 | 2 |
| **Total** | **33** | **166** | **69** |

No single task here can use 50 agents. The largest honest group is S1.1 at 16 (one per fuzz target), and the box can only run 3-4 fuzzers at once.
"Useful max" summed across sessions (69) is never needed at the same moment.

## 5. Realistic schedule at 16 concurrent agents (whole swarm) -> security share 3-5 slots

| When (Zagreb) | Security agents running | Work |
|---|---|---|
| Oct 20 evening (+02:00) | 2 | S0.1, S0.2 (already running) |
| Oct 21 | 4 | S6.1 memo; S1.1 wave A (hades token, hades config, review_plan); S3.1 starts (1 agent: shell-syntax family) |
| Oct 22 | 5 | S1.1 wave B (pair::verify, config, schema, nerve); S2.1 CodeQL; S3.1 (2 more families) |
| Oct 23 | 5 | S1.1 wave C (memory, hermes-tools jail, lan admit, galion-one); S2.2 (2 crate groups); S3.3 |
| Oct 24 | 5 | S1.1 wave D (rest); S2.2 (2 groups); S2.3 edge SAFETY; S3.4 |
| Oct 25 (CET +01:00 from 03:00) | 4 | S1.2, S1.3 CI; S2.4 Miri; S4.1 flake; S3.3 DST check runs across the change |
| Oct 26 | 4 | S1.4 merge; S2.2 last groups + S2.5; S4.2 (2 families); S4.3 |
| Oct 27 | 4 | S4.2 (2 families); S5.1 (3 agents) |
| Oct 28 | 3 | S5.2 on the merged candidate (2 independent runs); S5.3; S5.4 |
| Oct 29 | 2 | S5.5 after owner purge; S7 verifications |
| Oct 30 | 2 | S8.1 |
| Oct 21-30 | 1 (rolling) | S7 verifier, only when Slavik reports an owner step done |

Merge order (owner merges): ucpu 3e70ffb -> edge-boot 753676e -> integration-1 8fd26f6 (already contains launch-gates 79ee761, hades-approval ccaab42, sandbox d194c19)
-> sec-runbooks -> fuzz-sast -> fuzz-all (S1.4) -> SAST branch(es) -> HADES fixes (S3) -> sandbox/seccomp (S4) -> export cleanup (S5.1) last, because it edits docs and
fixtures that every other branch touches. S5.2 runs again after every merge. The history purge (S7.2) and the public repo creation happen **after** the final merge.

## 6. Items needing Slavik (security sector)
1. SEC-1 rotations in the stated P0 order (GitHub token, tunnel credential/token, root password first) and a push freeze window (S7.7).
2. SEC-1 history purge: approve `galion-purge` (Rust, std-only) and the plan "new public repo from the exported tree; private repo never flipped".
3. SEC-2, SEC-3 (staged script), SEC-4 (delete/move after key rotation), SEC-5 (option A recommended).
4. Licence ruling memo S6.1 (GPL/LGPL test tools, kernel syscall constants, libFuzzer NCSA, CodeQL terms).
5. Export scope: stack/, TEACHER/, admin.preferences, docs/launch/SEC-*.md; acceptance of any remaining public IP literal; the quick-tunnel hostname in docs/session-2026-10-18-evening.md:45.
6. GitHub settings after going public: branch protection, private vulnerability reporting, secret scanning + push protection, Dependabot alerts, code scanning (CodeQL), tag ruleset; `__CONTACT_EMAIL__`.
7. HADES real admin key pair: who generates it, where the seed lives offline, rotation/revocation policy.
8. A non-production test host for the privileged sandbox test (S4.4) and a real arm64 device/runner (S4.5).
9. `.cargo/config.toml` rustflags (dalek serial backend) vs any RUSTFLAGS in CI (integration-1 Needs #2): affects the SAST/fuzz workflows too (fuzz needs its own RUSTFLAGS for sanitizers; keep it in the separate fuzz workspace).

## 7. Top risks
1. **Rotation breaks the swarm and live services mid-sprint.** The GitHub token (row 1) is also the push helper's token and the box key (row 18) is the probe path; tunnel rotation touches all public hostnames. Without a planned freeze window, pushes fail and agents improvise (forbidden).
2. **Publishing the wrong history.** gitleaks shows 0 raw secrets, but history still holds `*.redacted` inventories, the cloudflared config and `contrib/`; the sec-runbooks docs themselves name secret paths and the tunnel id prefix. Flipping visibility instead of a new repo from the export (or forgetting to export-ignore docs/launch/SEC-*.md) leaks the infrastructure map.
3. **HADES admin-safety is a deny-list.** Substring matching over plan text is likely bypassable by shell indirection; a validly signed token plus a bypass gives `WouldApply`. Must be adversarially tested (S3.1) before HADES is wired to anything that executes.
4. **Owner bandwidth, not agents, is the bottleneck.** SEC-1..5, licence ruling, export scope and GitHub settings are ~10 owner decisions/actions in 10 days; the swarm cannot do them. Scorecard "Maintained"/"Contributors" will score 0 on Oct 31 regardless.
5. **CPU and request-limit contention.** Fuzzing and CodeQL are CPU-heavy on an 8-core box shared by ~16 agents; the request limit already killed 11 of 15 and later 2 planners. Long fuzz campaigns must wait for public-repo CI; unproven items (real arm64 seccomp, privileged sandbox test) may still be open on Oct 31 and must be stated as such, not claimed.
