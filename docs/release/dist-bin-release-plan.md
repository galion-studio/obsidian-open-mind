<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# dist/bin release plan (slot 445, prep for D-428-2)

**Short version:** the five prebuilt binaries that sat in `dist/bin/` are
removed from the tree on this branch (`git rm`, history untouched). Nothing
live used them: every live unit runs from `/opt/galion/rust/bin/`. They can be
rebuilt byte-for-byte with `scripts/build-dist-bins.sh`; if you want public
downloads, they go up as GitHub Release assets later, on your word.

## What changed on the branch

| Item | Before | After |
|---|---|---|
| `dist/bin/` | 5 binaries + 5 `.license` + `SHA256SUMS` (11 files, ~3.2 MB) | empty; old bytes listed in `docs/release/dist-bin-removed-manifest.txt` (size, sha256, git blob) |
| repo unit files (`deploy/galion-desk`, `galion-one`, `link-watch`, `boot/rendered/*`, `survival/guardian-tick`) | `ExecStart=…/openmind/dist/bin/<bin>` | `/opt/galion/rust/bin/<bin>` (what live already runs); `tuner` -> `/opt/galion/tuner/bin/tuner` |
| `galion-desk` default `hermes_bin` | `dist/bin/desk-hermes` | `/opt/galion/rust/bin/desk-hermes` (live sets `DESK_HERMES_BIN` to that already) |
| guard | none | `crates/repo-check/tests/no_dist_bin_binaries.rs`: fails if any ELF/NUL file lands in `dist/bin/` again |
| recipe | none | `scripts/build-dist-bins.sh` (builds only; never installs, uploads or publishes) |

## Reproducibility (box, 2026-10-31)

Two runs of `scripts/build-dist-bins.sh` from two different checkout paths
and two different target dirs gave identical sha256 for all five
(x86_64-unknown-linux-musl, `--locked`, `SOURCE_DATE_EPOCH` = commit time,
paths remapped to `/src`):

| binary | size (B) | sha256 (first 16) | old dist/bin size |
|---|---|---|---|
| desk-hermes | 1,290,256 | b625486317723850 | 1,427,792 |
| galion-desk | 757,704 | 2f8a9e2e7b6e2c88 | 403,056 |
| galion-one | 960,648 | 5a99f1f473213726 | 616,600 |
| link-watch | 785,336 | 4f848e2e468bb387 | 394,392 |
| room | 495,848 | 60be036500484128 | 344,304 |

They are not the old bytes: the old files were built from older source and an
unknown recipe, which is the point of replacing them with a recipe.

## Gates on the branch

`cargo fmt --check` ok; `cargo test -p repo-check` ok (3 new tests);
`scripts/public-export.sh --allow-missing-tools`: 0 FAIL (no binaries, no
Python, secrets/gitleaks clean, REUSE ok, cargo deny ok), SBOM skipped
(cargo-cyclonedx not installed on the box), 1 WARN (public IPv4 literals in
docs; standing owner item).

## Owner decisions

- **D-445-1** Land this branch (local only today; not pushed)? Rec: yes. It
  closes the tree half of D-428-2.
- **D-445-2** Publish the five binaries as Release assets? Rec: not now;
  nobody downloads them, and assets can be added at any later release by
  running the script and attaching `SHA256SUMS` + `RECIPE.txt`.
- History purge stays tied to D-428-1 (not while the repo is private).

Rollback: `git revert` the commit; the old bytes are in git history (blobs
in the manifest).
