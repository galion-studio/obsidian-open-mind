# Cargo SBOM license allowlist

- DESK-ID: openmind.B44
- Generated: 2026-10-19T16:29:19+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Tool: `cargo xtask sbom` (crates/xtask/src/sbom.rs). It replaced `scripts/sbom.sh` (bash + jq) on 2026-10-21 (swarm/ci-workflow-rust); same rule, same report, same JSON (verified identical on the 153-package graph, docs/real-tests/ci-workflow-rust.md).
- SBOM path: `dist/sbom-cargo.json` (default mode only)
- Draft CI: `.github/workflows/sbom.yml` (not pushed)

## Allowlist

A package passes only when its `license` field is an SPDX expression that can be satisfied from this list:

| SPDX id | allowed |
| --- | --- |
| MIT | yes |
| Apache-2.0 | yes |
| BSD-2-Clause | yes |
| BSD-3-Clause | yes |
| ISC | yes |
| Zlib | yes |
| Unicode-3.0 | yes |

`Unicode-3.0` is the only Unicode id on the list. `Unicode-DFS-2016` is not allowed. Exception ids are not on the list, so `Apache-2.0 WITH LLVM-exception` fails.

## Rule

- `OR` passes when one side passes. A `/` between license ids is treated as `OR` (`MIT/Apache-2.0`).
- `AND` passes only when every side passes.
- Parentheses are honored.
- `WITH` passes only when both the license id and the exception id are on the allowlist.
- A missing, empty, or unparsable license fails.
- Identifiers are case-sensitive (`mit` is not `MIT`).
- `MIT OR GPL-3.0` passes, because MIT is on the list. `GPL-3.0 OR MPL-2.0` fails.
- An empty package list fails.

The report is one row per package: status, workspace or dep, name, version, license, reason.

## What the script runs

Default mode, from the repo root:

```
nice -n 19 cargo metadata --format-version 1 --offline
```

`cargo xtask sbom` first runs `cargo fetch --locked` (fills the cache, never rewrites `Cargo.lock`), then writes `dist/sbom-cargo.json`. No `cargo install`, no jq, no Python.

Default mode was not run for this ticket. `Cargo.lock` was being restored by other agents. The license check was tested with `--check` on fixture JSON under `/tmp`. That path does not run cargo and does not write `dist/sbom-cargo.json`.

```
cargo xtask sbom --check FILE
```

## Draft workflow

`.github/workflows/sbom.yml` is a draft. It is `workflow_dispatch` only. It is not committed and not pushed. The job runs `cargo fetch --locked` to fill the runner cache (not `cargo install`, and `--locked` must not rewrite `Cargo.lock`), then `cargo metadata`; both now happen in the single step `cargo xtask sbom`.

## Fixture test

Ran from `/opt/galion/openmind`. Fixtures were not committed.

Allowed fixture, exit 0:

```
sbom: DESK-ID openmind.B44
sbom: time 2026-10-19T16:29:19+02:00
sbom: mode check
sbom: allow MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib Unicode-3.0
status	kind	name	version	license	reason
ok	dep	lic-and	1.0.0	MIT AND Apache-2.0	ok
ok	dep	lic-apache	1.0.0	Apache-2.0	ok
ok	dep	lic-bsd2	1.0.0	BSD-2-Clause	ok
ok	dep	lic-bsd3	1.0.0	BSD-3-Clause	ok
ok	dep	lic-choice	1.0.0	MIT OR GPL-3.0	ok
ok	dep	lic-isc	1.0.0	ISC	ok
ok	workspace	lic-mit	1.0.0	MIT	ok
ok	dep	lic-or	1.0.0	MIT OR Apache-2.0	ok
ok	dep	lic-paren	1.0.0	(MIT OR Apache-2.0) AND BSD-3-Clause	ok
ok	dep	lic-slash	1.0.0	MIT/Apache-2.0	ok
ok	dep	lic-unicode	1.0.0	Unicode-3.0	ok
ok	dep	lic-zlib	1.0.0	Zlib	ok
sbom: ok (12 packages)
```

Rejected fixture, exit 1:

```
sbom: DESK-ID openmind.B44
sbom: time 2026-10-19T16:29:19+02:00
sbom: mode check
sbom: allow MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib Unicode-3.0
status	kind	name	version	license	reason
FAIL	dep	lic-and-bad	1.0.0	(GPL-3.0 OR MPL-2.0) AND MIT	disallowed
FAIL	dep	lic-both-bad	1.0.0	GPL-3.0 OR MPL-2.0	disallowed
FAIL	dep	lic-empty	1.0.0	-	missing
FAIL	dep	lic-gpl	1.0.0	GPL-3.0	disallowed
FAIL	dep	lic-missing	1.0.0	-	missing
ok	dep	lic-mit	1.0.0	MIT	ok
FAIL	dep	lic-unlicense	1.0.0	Unlicense	disallowed
FAIL	dep	lic-with	1.0.0	Apache-2.0 WITH LLVM-exception	disallowed
sbom: FAIL (8 packages, 7 disallowed)
```

Empty `packages` array, exit 1. JSON with no `packages` array, exit 1 (`sbom: need cargo metadata format-version 1 with a packages array`).

After these checks `dist/sbom-cargo.json` was still absent. `Cargo.lock` mtime was unchanged (`2026-10-19 14:29:07.656190001 +0000`).
