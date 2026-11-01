<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: hermes-files-owner-20261101

| Crate | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| libc | 0.2 (0.2.191 in Cargo.lock) | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | galion-files: the open flags O_NOFOLLOW, O_NONBLOCK, O_NOCTTY (constants only, no unsafe); hermes-tools tests: umask(2) to reproduce the live UMask=0077 |

Not new to the workspace: galion-backup, galion-guard and openmind-sandbox already depend on it.
Chosen over hard-coding the flag values, which differ between architectures.
