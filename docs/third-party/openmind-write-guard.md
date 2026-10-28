<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: openmind-write-guard (slot 419)

No new third-party crates and no copied code. `galion-report-push` uses std
plus the in-repo `zagreb-time` crate (MIT, already in the workspace; it pulls
the existing `chrono` / `chrono-tz`, MIT OR Apache-2.0). It calls the system
`git` binary (GPL-2.0, run as a separate program, not linked or vendored),
exactly like `push-branch.sh` / `remote-push.sh` already do.

Research before writing (don't reinvent): `git2`/`gix` would add a large
dependency tree (libgit2 is C; gix is big) for five plumbing calls, and
neither offers a "refuse this work tree" policy; path canonicalisation is in
std. So a ~300-line std-only guard over the git CLI was the lighter choice.
