<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# conn-rate VPS replay fixture (slot 178, cortex-connrate-fix)

Three READ-ONLY reads of the VPS `/proc/net/tcp` + `/proc/net/tcp6` over ssh (`cat` only, nothing written),
30 s apart, first at 2026-10-21T09:24:42+02:00. Redacted on the box with `awk` before commit: only rows whose
local or remote port is 8642, 8091, 8420, 8646 or 8650; LISTEN rows dropped; uid, inode and timer columns zeroed;
non-loopback addresses would have been replaced by placeholders (there were none: every kept row is 127.0.0.1).

Used by `connrate::tests::replay_vps_sample_b41_vs_identity_vs_flow`. Measured (30 s windows w0, w1):

| port | identity /s | old B41 delta /s | b41flow /s |
|---|---|---|---|
| 8642 hermes-rs | 1.37, 1.37 | 0.00, 0.03 | 1.35, 1.43 |
| 8091 galion-one | 1.07, 1.03 | 0.00, 0.03 | 0.93, 1.08 |
| 8420 memory | 0.13, 0.10 | 0.00, 0.00 | 0.05, 0.12 |
| 8646 | 0.07, 0.07 | 0.00, 0.00 | 0.00, 0.07 |
| 8650 | 0.07, 0.07 | 0.00, 0.00 | 0.07, 0.07 |
