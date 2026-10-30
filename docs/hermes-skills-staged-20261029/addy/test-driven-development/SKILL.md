---
name: test-driven-development
description: Red, green, refactor. Write a failing test first, make it pass with the smallest change, then clean up.
---
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Test-driven development

Loop, one behaviour at a time:

1. Red: write one test for the next behaviour (`write_file` or `edit_file`). Run it with `terminal`
   (for Rust: `cargo test -q <name>`; shell scripts: a small `test.sh` with `set -e`). It must fail, and for the right reason.
2. Green: make the smallest change that passes it. Run the test again and show the pass.
3. Refactor: tidy names and duplication. Run all tests again; they must stay green.

Rules:
- Never edit a test to make wrong code pass. If the test is wrong, say why, then change it.
- Show the real test output; do not write "tests pass" without it.
- The terminal has no network. Tests that need the network can not run here; say so.
- Commands run in the workspace sandbox with a timeout (default 60 s, max 300 s); keep tests fast.
