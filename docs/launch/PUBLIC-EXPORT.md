<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Public export: what goes public on 2026-10-31 and how it is checked

Written 2026-10-20T20:00:00+02:00 (Europe/Zagreb) by Grok Bot, swarm slot 15
(`swarm/launch-gates`). Nothing here publishes anything or changes repository
visibility. Those steps are the owner's (see `docs/launch/GATES-OCT31.md`).

## The rule: publish the exported tree, never this repository's history

`.gitattributes` marks private material `export-ignore`. `git archive` (and
GitHub's own source tarballs) honour that, **but git history does not**. Every
excluded file is still in the history of `galion-studio/obsidian-open-mind`
(the two "Record ..." commits of 2026-10-19 brought in `stack/` and `contrib/`).
gitleaks over all 251 commits finds **no raw secret**, yet the history still
holds the `*.redacted` secret inventory, the cloudflared tunnel config, the
operator's `contrib/` material and committed binaries.

So the private repo must **not** be made public by flipping its visibility.
Recommended: create the public repository (or a fresh orphan branch in a new
repo) from the exported tree as a single root commit, then develop in public
from there. The private repo stays private as the archive. A history rewrite
of the private repo is the alternative; it is an owner decision, and
`git filter-repo` is Python, so it is not used by the swarm.

## What is excluded (`.gitattributes`)

| Rule | Files today | Why |
|---|---|---|
| `/contrib export-ignore` | 41 | vendored Grok Bot operator material: VPS ssh MCP, access skills, private notes |
| `*.redacted export-ignore` | 25 (22 in stack/kms, 3 in stack/hermes) | values are `REDACTED` markers, but names, paths and ids map the secret store |
| `/stack/kms export-ignore` | 30 | the key-management mirror: console key routes, cloudflared `config.yml` (tunnel id), `lock-secrets.sh` |
| `/stack/INDEX.md export-ignore` | 1 | lists every secret path and the sha256 of each secret-holding original |
| `/dist/bin export-ignore` | 0 (was 11) | the committed release binaries (+ `.license` companions) were `git rm`'ed on swarm/dist-bin-release-plan (slot 445, D-428-2): manifest `docs/release/dist-bin-removed-manifest.txt`, rebuild with `scripts/build-dist-bins.sh`; the line stays as a guard for local copies |
| `/edge/dist export-ignore` | 6 | same, for omnode |

Result at `17d7b73`: **738 of 830 tracked files exported, 92 export-ignored.**
The exported workspace still builds: `cargo check --workspace --locked` passes
on the exported tree (no crate depends on excluded paths; `Cargo.toml` already
excludes `contrib`).

## Producing and verifying the tree

```
scripts/public-export.sh --out /tmp/obsidian-open-mind-public [--ref REF] [--online]
```

Tools are taken from `$GALION_TOOLS_BIN` or `$PATH`. Every gate runs; exit 0
only when no FAIL-level gate failed. Output: `tree/`, `REPORT.md`,
`MANIFEST.sha256` (sha256 of every exported file), `sbom/*.cdx.json`,
`gitleaks.json`, `logs/`.

| # | Gate | Level | Tool |
|---|---|---|---|
| 1 | `git archive <ref>` honouring export-ignore | FAIL | git |
| 2 | path policy: no contrib/, `*.redacted`, stack/kms/, stack/INDEX.md, `.env`, `*.pem`, `*.key`, `id_*`, `*.sealed`, `auth.json`, `*.cookie`, `*.token`, `credentials*.json` ... | FAIL | find |
| 3 | no ELF / PE / Mach-O / Java class binaries | FAIL | magic bytes |
| 4 | no `*.py` | FAIL | find |
| 5 | built-in secret shapes (OpenRouter, Anthropic, OpenAI, GitHub classic + fine-grained, AWS, PEM private keys, Slack, Google, Hugging Face, Replicate, cloudflared TunnelSecret, JWT, `REDACTED:` markers); prints file:line only | FAIL | grep -E |
| 6 | `gitleaks dir --redact` | FAIL | gitleaks 8.30.1 (MIT) |
| 7 | REUSE 3.3 lint | FAIL | comply 0.1.1 (MIT OR Apache-2.0, Rust) |
| 8 | `cargo deny check licenses bans sources` (+ `advisories` with `--online`), workspace and `edge/openmind-node` | FAIL | cargo-deny 0.20.2 (MIT OR Apache-2.0) |
| 9 | CycloneDX 1.5 JSON SBOM per crate from `Cargo.lock` (`--all --target all --license-strict`) | FAIL | cargo-cyclonedx 0.5.9 (Apache-2.0) |
| 10 | public IPv4 literals and `*.trycloudflare.com` hostnames | WARN | grep -E |

CI: `.github/workflows/launch-gates.yml` runs the same script on every pull
request to main (read-only permissions, sha256-pinned tool downloads, report and
SBOM uploaded as an artifact).

Real-life test: `GALION_REAL=1 scripts/real/launch-gates.sh` (results in
`docs/real-tests/launch-gates.md`) exports HEAD, builds the exported tree, and
then plants canaries (a generated fake AWS key, a GPL crate, an ELF binary, a
GPL SPDX header, a removed contrib rule, a new `*.redacted` file) and checks
that every gate catches its canary.

## Still in the exported tree: owner decisions (gate 10 WARN and review)

These are not secrets, but they describe live infrastructure or internal
operations. Decide keep / edit / exclude before the first public commit:

- `docs/TERMINAL-LOG.md` lines 36, 48, 51: the box egress IPv4 that is on the
  VPS fail2ban `ignoreip` allow-list.
- `stack/hermes/SOUL.md:9` and `TEACHER/QUEUE.md:29`: the France server IPv4.
- `docs/session-2026-10-18-evening.md:45`: a `*.trycloudflare.com` quick-tunnel
  hostname (SEC-5 is about stopping that tunnel).
- `crates/repo-check/tests/checks.rs` 169/174: test-fixture IPs (fine).
- Internal operations material that is not code: `TEACHER/` (10), `stack/`
  (103 non-secret copies: agent personas, Hermes desk/runs notes, the
  `usr/local/bin/hermes` wrapper which names the secret env path), and
  `admin.preferences`. REVIEW.md recommends excluding all of `stack/`. Note
  that `galleon-arena` reads its seed homes from
  `stack/agents/personas/arena/homes/` at run time (`src/load.rs`), so a
  `/stack export-ignore` rule needs a `!`-style carve-out (git attributes have
  no negation, but `-export-ignore` unsets it; tested at 2026-10-20 20:03
  +02:00, this keeps exactly the 8 arena seed files):

  ```
  /stack/** export-ignore
  /stack/agents -export-ignore
  /stack/agents/personas -export-ignore
  /stack/agents/personas/arena -export-ignore
  /stack/agents/personas/arena/** -export-ignore
  ```

  or move the seeds out of `stack/`.
  `/TEACHER export-ignore` and `/admin.preferences export-ignore` are plain
  one-line additions.
