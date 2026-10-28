<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# real-tests: nerve-edge-overcount (slot 394)

Recorded 2026-10-28 16:02:03 +0100 (Europe/Zagreb).

## Commands

```
export CARGO_TARGET_DIR=/workspace/swarm/target/nerve-edge-overcount
GALION_REAL=1 GALION_REAL_VPS=1 cargo test -j 4 -p openmind-nerve \
  --test real_edge_overcount -- --nocapture
```

## Results

- `real_edge_overcount_engine_replay` PASS: failed_requests=900 rate=30.03/min window_s=1798
- `real_edge_overcount_vps_journal_replay` PASS: lines=370 once_failed=370 eng_failed=370
  (dest hosts scrubbed in-process; not stored in repo). Rate printed uses
  synthetic 1 s spacing for journal lines and is not a live rate estimate.

Ports 22640-22649 unused (pure logscan replay; no nerve serve).
NO VPS writes, NO deploy.
