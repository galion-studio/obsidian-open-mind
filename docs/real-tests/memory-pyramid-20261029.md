<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Memory pyramid real test (2026-10-29)

Box run of `openmind-memory` from this branch on 127.0.0.1:19431 with an
empty temp data dir (no production data).

1. `/capture` three turns: two in `demo-test-s1` (codeword, lighthouse plan
   10:25), one in `api-real1` (favourite colour).
2. `/recall` with `"pyramid": true` from a new test session
   (`demo-test-s3`, "What is my demo codeword?") returned
   `<long-term-facts>` with `my demo codeword is demo-test-aa11`.
3. The same question from a real session (`api-real2`) returned nothing:
   test material is hidden from real chats.
4. "What did we talk about earlier today about the lighthouse plan?" returned
   the fact plus a `<past-conversations window="today">` block with the
   time-stamped matching turn of `demo-test-s1`.
5. "What is my cat's name?" returned an empty context with
   `pyramid.facts = 0` (hermes-rs then injects the no-hit note).
6. Without `"pyramid"` the reply is unchanged (session scope only).
7. `/episodes {"query":"lighthouse plan today","session_key":"demo-test-x"}`
   returned the episode with start/end times in Europe/Zagreb.
8. `/search/memories "demo codeword"` (the MCP memory_search path) now finds
   the extracted L1 facts.
9. `pyramid-backfill` run twice on the same DB gave the same counts
   (idempotent).
