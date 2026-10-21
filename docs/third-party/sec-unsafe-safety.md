<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: sec-unsafe-safety (slot 229)

No new dependencies, and no copied code. The `safety` check is std-only.

The approach mirrors clippy's `undocumented_unsafe_blocks` and
`missing_safety_doc` lints (rust-clippy, MIT OR Apache-2.0,
https://github.com/rust-lang/rust-clippy). Those lints were used as the
reference oracle (same accepted comment positions), not as code.
Why not just the lint: clippy needs a full build of every crate on every
target, while repo-check scans the whole tree, including the separate
edge/ and sim/ workspaces, in under a second without compiling.
The landlock 0.4.7 crate (MIT OR Apache-2.0, already a dependency) was read
to confirm `restrict_self` doesn't allocate on its success path (finding F5).
