<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hermes-mem-growth (slot 198)

None. No new crates, no copied code. The heap profile uses a std-only
counting `GlobalAlloc` (in the `heap_growth` test binary only) instead of
dhat or jemalloc stats: it answers the one question asked (live bytes kept
per chat, before and after clearing one structure), needs no C code and no
extra dependency, and does not touch the shipped binary.
