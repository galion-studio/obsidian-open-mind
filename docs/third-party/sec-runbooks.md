<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# Third-party code and tools: swarm/sec-runbooks

Written 2026-10-20T20:44:00+02:00 (Europe/Zagreb).

## New Rust dependencies

**None.** `crates/history-purge` uses only the Rust standard library
(`[dependencies]` is empty; `Cargo.lock` gains only the crate's own entry).
No code was copied from other projects. The fast-export stream grammar was
implemented from the git-fast-import(1) manual ("Input format").

## External programs used (executed, not linked, not vendored)

| Tool | Version on the box | Licence (SPDX) | Source | Used for | Why not write it / why this one |
|---|---|---|---|---|---|
| git (`fast-export`, `fast-import`, `ls-tree`, `cat-file`, `gc`) | 2.47.3 | GPL-2.0-only | https://git-scm.com | runtime of `galion-purge`; it is invoked as a separate process, the same way every script in the repo calls git, so its licence does not extend to our code | git's own exporter/importer is the reference implementation of the object format; reimplementing pack and object writing would be far more code and risk |
| gitleaks | 8.30.1 (`/workspace/swarm/target/launch-gates/tools/bin/gitleaks`, fetched by swarm/launch-gates with a sha256 pin) | MIT | https://github.com/gitleaks/gitleaks | real-life test only: secret scan of the scratch history before and after the purge | same scanner the launch gates and CI use |

## Considered and rejected

| Candidate | Licence | Reason |
|---|---|---|
| git-filter-repo | MIT and GPL-2.0 parts | Python (swarm rule: no Python) |
| BFG Repo-Cleaner | GPL-3.0-only | licence not on the allow-list; JVM |
| gitkat / gitkat-rewrite 0.5.2 (https://github.com/Aureuma/GitKat) | MIT | allowed, but young (0.x, about 170 downloads), pulls the gitoxide tree; our need is one narrow operation |
| nornir-git 0.2.1 (https://codeberg.org/nordisk/nornir) | MIT OR Apache-2.0 | allowed, library only and young; would still need the CLI and verification |
