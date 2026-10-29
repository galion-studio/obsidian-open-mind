---
name: spec-driven-development
description: Write a short spec (inputs, outputs, rules, examples, checks) before writing code, and build against it.
---
<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Spec-driven development

1. Write `SPEC.md` in the workspace with `write_file`:
   - Goal (one sentence).
   - Inputs and outputs, with types and limits.
   - Rules and edge cases (empty input, too large, bad format).
   - 3 or more concrete examples: input, expected output.
   - Out of scope.
2. Turn each example into a test before the code (see test-driven-development).
3. Build the smallest code that meets the spec. Change the spec first if you find it wrong, and say so.
4. Done means every example in the spec passes with `terminal`, and you show that output.

Keep the spec under one page. If the admin's request is unclear, ask one question instead of guessing in the spec.
