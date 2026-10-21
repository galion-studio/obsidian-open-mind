<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life test: launch-gates (slot 15)

> **2026-10-21T02:40:00+02:00:** `scripts/real/launch-gates.sh` is gone; its Rust port is `crates/repo-check/tests/real_launch_gates.rs`. New run: [realscript-port-2.md](realscript-port-2.md). The record below is historical.

Run on the box, 2026-10-20 (Europe/Zagreb, +02:00), at commit `17d7b73`.
No network service is involved, so no port in 19150-19159 was opened and no
live VPS probe applies; the real thing under test is the public-export
pipeline and the third-party scanners on the real repository. Nothing was
pushed, published or changed on the VPS.

Tools (`/workspace/swarm/target/launch-gates/tools/bin`): gitleaks 8.30.1,
cargo-deny 0.20.2, cargo-cyclonedx 0.5.9, comply 0.1.1.

## 1. `GALION_REAL=1 scripts/real/launch-gates.sh`

```
== A. real export of HEAD (2026-10-20T20:02:17+02:00)
  ok   public-export HEAD exit 0
     | 1 git archive | PASS | 738 files exported of 830 tracked (92 export-ignored) |
     | 2 path policy | PASS | no contrib/, *.redacted, stack/kms/, stack/INDEX.md, key/env/cookie/token files |
     | 3 no binaries | PASS | no ELF/PE/Mach-O/class files |
     | 4 no Python | PASS | 0 *.py |
     | 5 secret patterns (built-in) | PASS | 0 hits over 14 token shapes |
     | 6 gitleaks dir | PASS | gitleaks 8.30.1: no leaks |
     | 7 REUSE lint (comply) | PASS | 723 total, 683 passed, 0 failed, 40 skipped |
     | 8 cargo deny | PASS |  Cargo.toml: bans ok, licenses ok, sources ok; edge/openmind-node/Cargo.toml: bans ok, licenses ok, sources ok; |
     | 9 SBOM (CycloneDX 1.5) | PASS | 36 files in sbom/; 130 unique components, 0 without licence data |
     | 10 infra disclosure | WARN | 7 public IPv4 literals (logs/public-ipv4.txt), 1 quick-tunnel hostnames (logs/quick-tunnels.txt); owner decides |
== B. cargo check of the exported tree (2026-10-20T20:02:21+02:00)
  ok   exported workspace: cargo check --workspace --locked
== C. canary run (2026-10-20T20:02:32+02:00)
     | 1 git archive | PASS | 783 files exported of 835 tracked (52 export-ignored) |
     | 2 path policy | FAIL | 40 forbidden paths, see logs/bad-paths.txt |
     | 3 no binaries | FAIL | 1 executable binaries, see logs/binaries.txt |
     | 4 no Python | PASS | 0 *.py |
     | 5 secret patterns (built-in) | FAIL | 1 hits (file:line rule in logs/secret-patterns.txt, values not printed) |
     | 6 gitleaks dir | FAIL | gitleaks 8.30.1: 2 findings (redacted) in gitleaks.json |
     | 7 REUSE lint (comply) | FAIL | 3 problems, see logs/reuse-lint.txt |
     | 8 cargo deny | FAIL |  Cargo.toml: bans ok, licenses FAILED, sources ok; edge/openmind-node/Cargo.toml: bans ok, licenses ok, sources ok; |
     | 9 SBOM (CycloneDX 1.5) | PASS | 37 files in sbom/; 130 unique components, 0 without licence data |
     | 10 infra disclosure | WARN | 7 public IPv4 literals (logs/public-ipv4.txt), 1 quick-tunnel hostnames (logs/quick-tunnels.txt); owner decides |
  ok   canary export exits non-zero (1)
  ok   canary caught by gate 2 path policy
  ok   canary caught by gate 3 no binaries
  ok   canary caught by gate 5 secret patterns
  ok   canary caught by gate 6 gitleaks dir
  ok   canary caught by gate 7 REUSE lint
  ok   canary caught by gate 8 cargo deny
  ok   *.redacted canary export-ignored
  ok   contrib/ appears once its rule is removed (gate 2 is the backstop)
== result (2026-10-20T20:02:36+02:00): 11 ok, 0 failed
```

## 2. Export with advisories (`scripts/public-export.sh --online`)

```
- started: 2026-10-20T20:02:37+02:00
- ref: `HEAD` = `17d7b73d222abf1fcc692349221166b98476fb54`
- tree: `/workspace/swarm/target/launch-gates/public-export/tree`
| gate | result | detail |
|---|---|---|
| 1 git archive | PASS | 738 files exported of 830 tracked (92 export-ignored) |
| 2 path policy | PASS | no contrib/, *.redacted, stack/kms/, stack/INDEX.md, key/env/cookie/token files |
| 3 no binaries | PASS | no ELF/PE/Mach-O/class files |
| 4 no Python | PASS | 0 *.py |
| 5 secret patterns (built-in) | PASS | 0 hits over 14 token shapes |
| 6 gitleaks dir | PASS | gitleaks 8.30.1: no leaks |
| 7 REUSE lint (comply) | PASS | 723 total, 683 passed, 0 failed, 40 skipped |
| 8 cargo deny | PASS |  Cargo.toml: advisories ok, bans ok, licenses ok, sources ok; edge/openmind-node/Cargo.toml: advisories ok, bans ok, licenses ok, sources ok; |
| 9 SBOM (CycloneDX 1.5) | PASS | 36 files in sbom/; 130 unique components, 0 without licence data |
| 10 infra disclosure | WARN | 7 public IPv4 literals (logs/public-ipv4.txt), 1 quick-tunnel hostnames (logs/quick-tunnels.txt); owner decides |
- finished: 2026-10-20T20:02:41+02:00
- result: 0 FAIL, 1 WARN
- manifest: `MANIFEST.sha256` (738 files)
```

The tree is kept on the box at
`/workspace/swarm/target/launch-gates/public-export/tree` (738 files) with
`REPORT.md`, `MANIFEST.sha256`, `sbom/` (36 CycloneDX files), `gitleaks.json`
and `logs/`.

Gate 10 WARN details (file:line; these are infrastructure IPs and a tunnel
hostname, not secrets; owner decides, see docs/launch/PUBLIC-EXPORT.md):

```
TEACHER/QUEUE.md:29
crates/repo-check/tests/checks.rs:169
crates/repo-check/tests/checks.rs:174
docs/TERMINAL-LOG.md:36
docs/TERMINAL-LOG.md:48
docs/TERMINAL-LOG.md:51
stack/hermes/SOUL.md:9
docs/session-2026-10-18-evening.md
```

## 3. Whole private repository

| Check | Result |
|---|---|
| `comply lint .` (REUSE) | 812 total, 771 passed, 0 failed, 41 skipped (licence-text files). Before this branch: 1181 problem lines (no LICENSES/, no REUSE.toml) |
| `gitleaks git --redact .` (full history) | 251+ commits, ~6.5 MB, no leaks found |
| `gitleaks dir --redact .` (working tree incl. contrib/ and *.redacted) | no leaks found |
| `cargo deny check` (workspace, with advisories) | advisories ok, bans ok, licenses ok, sources ok (warnings: path wildcards, duplicate hashbrown) |
| `cargo deny check` (edge/openmind-node) | advisories ok, bans ok, licenses ok, sources ok (warning: duplicate syn) |
| `cargo deny check licenses` on comply's own dependency graph | licenses ok |
| `cargo fmt --all -- --check` | ok |
| `cargo clippy -j 4 --workspace --all-targets --locked -- -D warnings` | ok |
| `cargo test -j 4 --workspace --locked` | 121 suites, 497 passed, 0 failed |
| `scripts/self-check.sh`, `scripts/no-python.sh`, `scripts/spdx-scan.sh` | ok, ok, 175 scanned / 0 missing |

## Notes

- The SBOM `metadata.timestamp` is written by cargo-cyclonedx in UTC (zone letter suffix),
  as the CycloneDX schema allows; it is a generated artefact, not a doc, and
  is not committed. `bom-ref` values carry the local build path.
- SBOM licence expressions found across all components:
  `MIT OR Apache-2.0` (728), `MIT` (97), `Apache-2.0 OR MIT` (80),
  `(MIT OR Apache-2.0) AND Unicode-3.0` (27), `Unlicense OR MIT` (23),
  `MIT/Apache-2.0` (22), `Zlib` (19), `BSD-2-Clause OR Apache-2.0 OR MIT` (10),
  `BSD-3-Clause` (3), and one each of `Apache-2.0/MIT`,
  `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`,
  `MIT OR Apache-2.0 OR BSD-1-Clause`, `MIT OR BSD-3-Clause`. All are
  satisfiable from the RULES.md allow-list.
