<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Release artefacts (integration-ci S4-T7, slot 216, 2026-10-21 CEST +02:00)

Base: integration-5 0d68f46. Branch: swarm/ci-release-artefacts.
Still LOCKED: release.yml runs only on a `v*` tag push, and its release job
only when the repo is public AND `vars.RELEASE_ENABLED == 'true'`. No tag was
created or pushed; nothing was published.

## What release.yml does now

| step | before (0d68f46) | now |
|---|---|---|
| build | `cargo build --release --workspace --locked` (host glibc, dynamic) | `scripts/ucpu-gate.sh build static size` for x86_64 + aarch64 musl (rust-lld; zig cc for bundled SQLite), workspace + omnode; static and 2 MiB checks |
| provenance | none | `SOURCE_DATE_EPOCH`, `GALION_BUILD_COMMIT` (12-char sha), `GALION_BUILD_TIME` (commit time, Europe/Zagreb `+02:00`/`+01:00`) |
| package | shell loop over 5 hand-picked names, one x86_64 tar.gz | `galion-release package` per target: every release ELF (36 per target), MANIFEST (sha256, size), LICENSE, README.md; deterministic tar.gz |
| SBOM | after SHA256SUMS (not covered) | before SHA256SUMS (covered) |
| sums | `sha256sum ./*.tar.gz` | `galion-release sums dist`, then `sha256sum -c` as an independent check |
| rehearse | none | `galion-release rehearse dist`: verify + extract + run every binary with `--version` (aarch64 under `qemu-aarch64-static`) |
| sign / publish | cosign keyless + `gh release create` | unchanged (now also signs both archives + SBOM + SHA256SUMS) |

Asset names: `obsidian-open-mind-<tag>-<target>.tar.gz` (top directory of the
same name), `obsidian-open-mind-<tag>.cdx.json`, `SHA256SUMS`, plus the
`.sigstore.json` bundles.

## galion-release (crates/galion-release, std + miniz_oxide)

```
galion-release package --target T --tag vX.Y.Z --out DIR --bin-dir D [--bin-dir D]... [--extra F]...
galion-release sums DIR
galion-release verify DIR
galion-release rehearse DIR --work DIR [--expect-version X.Y.Z] [--qemu-dir DIR]
```

Release gate per binary (same rules as `ucpu-gate.sh static size`, read in
Rust from the ELF headers): `e_machine` matches the target, no `PT_INTERP`, no
`DT_NEEDED`, no `.symtab`, size <= 2,097,152 bytes. Any failure lists every
offender and exits 1; nothing is written.

* Archive: ustar, uid/gid 0 `root`, one mtime (`SOURCE_DATE_EPOCH`), sorted
  entries, gzip level 9 with MTIME 0. Same inputs -> same bytes (tested).
* `verify`: every file in SHA256SUMS re-hashed; every `.tar.gz`/`.cdx.json`
  in the dir must be listed; each archive is gunzipped (CRC + ISIZE),
  untarred (header checksums, no `..`/absolute paths/links), checked against
  its MANIFEST and run through the release gate again.
* `rehearse`: runs `<bin> --version` with an empty environment
  (PATH only), stdin closed, cwd/HOME in a scratch dir, 30 s timeout. First
  stdout line must be `<bin> <version>` or `<bin> <version> (...)` where
  version = the tag without `v` and pre-release suffix. So a tag that does not
  match the workspace version (e.g. `v0.1.1` while Cargo says 0.1.0) fails
  the release before anything is signed.
* Default exclude (test helpers, not product): `gaia-dummy`,
  `galion-bench-mock`, `hermes-mock-model` (D-216-2).

## Fixes needed to make this pass on integration-5

1. **galion-voice-serve was not stripped** (`.symtab`, 693,080 B x86_64):
   with a `[profile.release.package.galion-voice-cpu]` override on the
   binary's own package, cargo 1.99 passes `-C strip=debuginfo` instead of the
   profile's `strip = true` (measured with `cargo build -v`). Both
   `ucpu-gate.sh size` and the new gate failed. Fix: `strip = true` in that
   override -> `-C strip=symbols`, 572,936 B (-17 %), no `.symtab`.
2. **21 binaries had no `--version`**; some (hermes-gateway) ignore unknown
   arguments and would have *started serving* on their default port when
   probed. Added a 4-line first-thing-in-main `--version`/`-V` handler
   (prints `<bin> <version>`, exits 0, no config/bind/I/O) to: bench-banking,
   bench-scoreboard, gaia, galion-backup, galion-health, galion-realtest,
   galion-vending, galion-voice-serve, galion-wallstreet, galleon-arena,
   guardian-tick, hades-gate, hermes-bench, hermes-gateway, one-defence,
   one-set-model, openmind-discovery, openmind-mcp-stack, persona-plex-relay,
   repo-check, omnode (omnode keeps its `version` subcommand too).

## Results (box, 2026-10-21 21:20 CEST +02:00)

See docs/real-tests/ci-release-artefacts.md. Summary: 2 archives x 36
binaries, all static/stripped/right machine/within budget; `verify` OK and
`sha256sum -c` OK; byte-identical on a second packaging; 72/72 `--version`
PASS (36 native x86_64, 36 under qemu-aarch64-static); tuner and
openmind-nerve started from both packages answer `/health` 200.

Sizes vs budget (largest): hermes-gateway x86_64 2,082,048 B = **99.3 %**
(15,104 B headroom), aarch64 89.5 %; openmind-memory 86.5 % / 79.7 %;
galion-guard 84.2 % / 67.5 %. Archives: x86_64 15,964,799 B, aarch64 15,294,802 B (gzip -9).

## Not done / out of scope

* No GitHub run: the release job is gated off and no tag may be pushed.
  The workflow is checked by `crates/galion-release/tests/workflow.rs`
  (trigger = `v*` tags only, gate condition unchanged, read-only top-level
  token, all actions SHA-pinned, packaging via galion-release, SBOM before
  SHA256SUMS before signing).
* armv7 / Android artefacts: tier 2, not packaged (plan says tier-1 only).
* The self-eval `ucpu-tier1` job's `grep` stage fails on integration-5 (398
  hits in vendor/candle-core, integration-5 blocker 0); release.yml's own
  build step runs `build static size` only and relies on self-eval for grep.

## Owner decisions

* **D-216-1** Ship all 36 binaries per target (incl. bench-*, galion-realtest,
  repo-check, galion-release) or a product subset? Recommended: all for
  v0.1.x (one tarball per arch is simplest; bench/test tools are small).
* **D-216-2** Keep the 3 test helpers excluded (recommended) or ship them.
* **D-216-3** hermes-gateway is at 99.3 % of the 2 MiB budget on x86_64: the
  next feature breaks the gate. Options: trim (e.g. SQLite flags), or raise
  the budget for this binary only.
* **D-216-4** Provenance: only the 11 galion-common users print
  `(commit ..., built ...)`; the 21 new handlers and nerve/queue/guard print
  `<bin> <version>` only. Move them to `galion_common::build_info` (adds the
  galion-common dep to ~20 crates) or leave as is.
* **D-216-5** Archive layout: one tarball per target (now) vs one per
  binary per target (72 assets).
