<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: swarm/flake-fix

None. This branch adds no dependency and copies no code. It only changes
tests, one example (`openmind-sandbox/examples/escape.rs`), two docs lines
and adds `scripts/real/flake-fix.sh`. `Cargo.lock` is unchanged.

The Android `PTRACE_SEIZE` value (0x4206) is the Linux kernel UAPI constant
from `include/uapi/linux/ptrace.h`; a single number, not copied code.
