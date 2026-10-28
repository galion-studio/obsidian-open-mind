<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve restore-test component (wave-4 slot 378)

No new dependencies. `openmind-nerve` keeps using its existing crates (serde_json, std).
The log reader is a small, specific tail parser for the `galion-backup restore-test` /
`galion-restore-test.sh` output format, so no library fits better than ~200 lines of
in-house code (checked: the job is "read 64 KiB tail, match a few fixed lines"; a
generic log-parsing crate would add size for nothing). Tests use only the system
`tar`, `gzip`, `sha256sum`, `find`, `xargs`, `touch` and `bash` already on the box/VPS.
