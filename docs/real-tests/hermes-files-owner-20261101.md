<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes files tools: owner 65534 + 0644/0755 (live, 2026-11-01, times CET +01:00)

Approved by Mate for Slavik, card t554s2721, 16:02 CET. Code commit 63ae6aff (on 4de4a982).

Box: galion-files + hermes-tools + hermes-rs + hermes-gateway tests 259 passed, 0 failed,
1 ignored (CI=1); clippy -D warnings (all targets) and fmt clean; no-python ok.
musl release (zig cc + rust-lld, as 4de4a982): hermes-gateway 3,408,736 B,
sha256 7eecc8f44039c001e7cdd5998007388dee8c72244b1d16b94c8853b70ab9cf32.

VPS harness (root, TMPDIR=/var/tmp/files-owner-20261101/tmp, 16:14 CET): galion-files
tests/owner.rs 6/6 (incl. root_hands_to_65534_and_skips_foreign_files, links_are_never_followed),
hermes-tools tests/files_owner.rs 2/2 (files_write/files_mkdir/files_move through the tool path
give 65534:65534 0644/0755 under umask 0077).

Deploy: /opt/galion/rust/bin/hermes-gateway-63ae6aff, drop-in 99o-files-owner-63ae6aff.conf
(99m's [Service] block with the new ExecStart). One restart 16:15:10 CET; health 200, NRestarts 0,
/proc/PID/exe = the new binary, env HERMES_RS_TOOLS_WORKSPACE_MB=8192, GUARD_SCRIPT_MODE=1,
log "files: ... owner 65534:65534".

One-time unlock 16:15:43 CET: 376 root-owned items (338 files, 38 dirs) saved to
/var/backups/galion-files-owner-20261101.tsv, then chmod 0644/0755 and chown -h 65534:65534.
Not readable by 65534 before: 14 (12 files 0600, 2 dirs 0700; 11 visible as 65534). After: 0.
Root-owned after: 0. The 14 uid-1000 items were left alone.
