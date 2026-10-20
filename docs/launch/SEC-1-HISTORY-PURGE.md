<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# SEC-1 (part 2): history purge runbook and the `galion-purge` tool

Written 2026-10-20T20:44:00+02:00 (Europe/Zagreb) on `swarm/sec-runbooks`.
**Prepared and tested on a scratch copy on the box only.** Nothing was
pushed, no GitHub repository was rewritten, nothing ran on the VPS.

Do this **after** the rotations in `SEC-1-ROTATION.md`: once every value is
rotated, whatever is left in an old clone is dead, and the purge becomes
hygiene instead of the only protection.

## Which repositories

| Repo | What is in its history | Recommended action |
|---|---|---|
| `galion-studio/obsidian-open-mind` (private) | no raw secret (gitleaks over all 261 commits: 0 findings), but the redacted secret inventory (`*.redacted`, `stack/kms/`, `stack/INDEX.md`), operator material (`contrib/`), committed binaries (`dist/bin/`, `edge/dist/`): 89 paths, 92 blob ids | **LAUNCH-1 as planned**: publish a *new* repo from the exported tree (fresh history, `docs/launch/PUBLIC-EXPORT.md`) and keep this one private. A purge of this repo is optional; if wanted, use the procedure below with `docs/launch/purge-paths.obsidian-open-mind.txt` |
| the 28 `galion-src` clones (`/opt/galion/galion-src/*`, GitHub `galion-studio/*`) (O1) | possibly tracked `.env` files and key files; not verified by the swarm (needs `git` inside the clones on the VPS) | run the read-only O1 command at the end of `SEC-1-ROTATION.md`; for every repo with hits, write a purge list (`.env`, `.env.backup`, ...) and run the procedure below on a mirror of **the GitHub repo**, not on the VPS worktree |

## Tool choice

| Option | Licence | Language | Verdict |
|---|---|---|---|
| git-filter-repo | MIT and GPL-2.0 parts (see upstream COPYING) | **Python** | excluded: no-Python rule |
| BFG Repo-Cleaner | **GPL-3.0** | Scala/Java (JVM) | excluded: licence and JVM |
| `git filter-branch` | GPL-2.0 (part of git) | shell | works but deprecated by git itself (slow, footguns); no dependency added, but no verification either |
| GitKat (`gitkat`, `gitkat-rewrite` 0.5.2) | MIT | Rust (gitoxide) | permissive, but 0.x, about 170 downloads, a large gix dependency tree, ships pip/npm wrappers; not needed for "drop these paths" |
| nornir-git 0.2.1 (`purge_paths`) | MIT OR Apache-2.0 | Rust | library only, young; would still need a CLI and verification around it |
| **`galion-purge` (this branch, `crates/history-purge`)** | MIT | Rust, **zero dependencies** (std only) | chosen: about 1,170 lines of code and tests (std only) that drive git's own `fast-export`/`fast-import` (git is already installed everywhere this runs; it is executed as a program, not linked), with built-in safety checks and verification |

How it works: `git fast-export --all --no-data --signed-tags=strip
--tag-of-filtered-object=rewrite --reencode=no --fake-missing-tagger
--use-done-feature` is piped through a byte-exact filter that drops only the
`M`/`D` file-change lines whose path matches the purge list (data blocks such
as commit messages are copied by byte count and never parsed), into
`git fast-import --force` in the same repository. Then `reflog expire
--expire=now --all` and `gc --prune=now`. With `--no-data` blobs are not
streamed; fast-import reuses the existing objects, so it is fast.

Safety built in:

- `rewrite` works only on a **bare** repository (`git clone --mirror`),
  refuses shallow clones, refuses any path under `/opt/galion`, needs
  `--confirm-scratch-copy`, and refuses a repository that still has a remote
  unless `--strip-remotes` is given, which deletes the `remote.*` config
  (not refs) **before** rewriting. The result therefore cannot be pushed by
  accident.
- Output is paths, counts and object ids only; it never reads blob contents.
- After rewriting it re-scans every tree of every commit of every ref, and
  checks with `cat-file --batch-check` that the old blob ids of dropped paths
  are gone from the object store. A blob that is still present is reported
  as *shared* if a kept path has byte-identical content (on this repo: two
  MIT `LICENSE` texts), and as a failure otherwise.
- Writes `galion-purge-commit-map.tsv` (old commit id, TAB, new commit id)
  into the repo dir, so docs and swarm branches that cite old SHAs (for
  example base `13a296c`) can be mapped.
- Renames/copies (`R`/`C`) are never requested; if a stream ever contained
  one touching a listed path, the run stops instead of guessing.
- Empty commits (a commit that only touched purged paths) are kept, so the
  commit count and the merge structure stay the same.

Limits (by design): it drops whole paths; it does not replace text inside
kept files (no `--replace-text`); it strips signatures of signed tags; old
commit ids change for every commit after the first dirty one.

## Procedure (owner, Oct 27 to 30)

On the box or the owner's laptop (never on the VPS, which is production):

```
# 0. build (or use the release binary from CI)
cargo build --release -p history-purge
P=target/release/galion-purge

# 1. scratch mirror + an untouched backup
git clone --mirror https://github.com/galion-studio/<repo>.git scratch.git
tar -C . -czf <repo>-mirror-backup-$(date +%Y%m%d).tgz scratch.git

# 2. read-only: what would go
$P scan --repo scratch.git --paths purge-list.txt

# 3. rewrite the scratch copy (removes the remote config first)
$P rewrite --repo scratch.git --paths purge-list.txt --confirm-scratch-copy --strip-remotes

# 4. prove it
$P verify --repo scratch.git --paths purge-list.txt          # exit 0 = no listed path anywhere
git -C scratch.git log --all -m --no-renames --format= --name-only | grep -E '<same patterns>'   # empty
gitleaks git scratch.git --redact                            # 0 findings
git clone -q scratch.git tree && (cd tree && cargo check --workspace --locked)
```

Step 5 is the irreversible one and is the owner's decision alone:

- **Recommended for obsidian-open-mind:** do not push the rewrite. Create the
  public repo from the export (LAUNCH-1). Keep the private repo as the archive.
- **If the private repo itself must be cleaned:** GitHub keeps
  `refs/pull/*` (read-only, cannot be rewritten by push) and cached views of
  old commits. Steps: temporarily allow force pushes on the protected
  branches, then
  `git -C scratch.git push --force <url> 'refs/heads/*:refs/heads/*' 'refs/tags/*:refs/tags/*'`
  (explicit refspecs; `--mirror` would try to push `refs/pull/*` and fail),
  delete the old branches that no longer exist, re-enable protection, then
  ask GitHub Support to purge cached views and dereference old PR refs (see
  GitHub's "Removing sensitive data from a repository" guide). Every other
  clone must then be discarded and re-cloned: the box worktrees
  (`/workspace/swarm/*`, `/workspace/om*`), and, in a maintenance window,
  the VPS working copy `/opt/galion/openmind` (live services run from its
  `dist/`; the swarm never touches it). Open swarm branches based on old
  SHAs are rebased by the owner using the commit map.
- **galion-src repos:** same two choices per repo; for repos that are not
  going public, rotation alone (part 1) is enough and the purge is optional.

## Tested on the box (2026-10-20, scratch copy of base.git)

See `docs/real-tests/sec-runbooks.md` for the full log. Summary: 262 commits
(261 + one canary commit planted on a scratch-only branch), 27 refs; 91 listed
paths before, 0 after; 92 old blob ids gone, 2 shared LICENSE blobs kept;
gitleaks 2 findings (the canary) before, 0 after; plain-git cross-checks 0;
`git fsck --full` clean; commit count, refs and merges unchanged; the
rewritten `main` checks out without `contrib/`, `stack/kms/`, `dist/bin/`,
`edge/dist/` and `cargo check --workspace --locked` passes; the source
mirror's refs are byte-for-byte unchanged.
