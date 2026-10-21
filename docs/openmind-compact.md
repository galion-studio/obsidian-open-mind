<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# openmind-compact: model-free context compaction

Written 2026-10-21 17:45 +02:00 (Europe/Zagreb), branch `swarm/context-compact`, base
`aa77864` (live nerve build, contains hermes-rs and openmind-memory). Research and
licence survey: `/workspace/swarm/plans/research-context-compaction.md` on the box.
**Not wired into anything live.**

## Why

`hermes-rs` `fit_context()` keeps the system prompt plus the newest messages that fit
(65 536 − 4 096 tokens), so anything older is lost. `openmind-memory`
`POST /v2/offload/compact` answers 501. The paid Hermes model is off and the hermes-rs
spend cap is hit, so compaction must be CPU-only, model-free and cheap.

## Algorithm (one pass, deterministic, zero dependencies)

1. **Units.** Each message is split into non-empty lines. A fenced ``` block is one unit
   (never cut). Lines over 600 bytes are cut at `. `, `; `, `, ` or a space (UTF-8 safe).
2. **Hard anchors.** System messages and the latest `keep_last_turns` (2) turns (a turn
   starts at a user message) are kept verbatim. If they do not fit, it falls back to 1 turn,
   then to just the last user message (`stats.over_budget` says when even that is too big).
3. **Dedup, newest first.** Exact: a hash of the lowercased, whitespace-collapsed unit.
   Near: 64-bit SimHash over unigrams and bigrams, with numbers folded to one placeholder,
   at Hamming ≤ 3. Lookup uses a 4×16-bit banded index (pigeonhole, so no candidate is
   missed). A near-duplicate must also have **the same set of non-volatile IDs**:
   heartbeats that differ only in timestamp (`2026-10-21T08:00:01+02:00`, `+0200`) merge,
   but `port 22041` and `port 22042` never do.
4. **Score** each remaining unit: `1.0·BM25(query)/max + 0.35·recency + 0.2·meanIDF/max`,
   plus `+0.25` if it carries an ID or number, `+0.1` for code, and `+1.0` if it carries a
   *rare* ID (df ≤ max(3, 1 % of units)) that the anchored window or the query still
   mentions ("referenced"). The query defaults to the last user message. Recency is
   `0.5^((1-p)/0.25)` over message position.
5. **Line cohesion.** The pieces of one long source line share the best piece's score
   (×0.95), so a relevant line stays whole.
6. **Greedy fill** by score (ties: newer first) with conservative accounting. Each unit
   costs its tokens, plus a newline, plus a possible gap marker. The first unit of a
   message also pays the message overhead (4), a trailing marker, and a stub for every
   tool-call partner not yet emitted. So output ≤ budget whenever the anchors fit
   (property-tested).
7. **Assemble** in source order. Elided runs become `…`. Pieces of a split line are
   re-joined with their original separator. An assistant `tool_calls` message and its
   `tool` results are always emitted together (a partner with nothing selected gets
   `[compacted]`), so the transcript stays API-valid.

Token estimate: bytes/4 rounded up per message + 4, the same scale as hermes-rs
(`json.len()/4 + 4`) and openmind-memory `count_tokens`. The estimate sits behind one
function (`text::est_tokens`), so an exact counter (`tiktoken-rs`, MIT) can replace it later.

Hot loops do not allocate once warm: the term arena, unit table, DF map, dedup index,
order and score buffers live in `Compactor` and are cleared, not freed, between calls.
Words are borrowed slices. Hashes use a fixed FxHash-style mix plus a splitmix finaliser.
The `HashMap`s use an identity hasher over those pre-mixed keys, with no random seed, so
the output is bit-identical run to run (tested).

### Plug points (for a model later; none used now)

* `Rescorer::rescore(query, units, scores)`: e.g. an LLMLingua-2 token classifier
  (MIT/Apache-2.0 weights) on candle, applied only to the units that survived dedup.
* `GapCompressor::compress_gap(elided, max_tokens)`: replaces a `…` marker with a short
  summary. It is capped at the marker's reserved size, so the budget guarantee holds; for
  bigger summaries, lower `budget_tokens` to reserve room.

## API

```rust
use openmind_compact::{Compactor, Config, Message, Role};
let mut c = Compactor::new(Config { budget_tokens: 16_000, ..Default::default() });
let out = c.compact(&messages, None);          // query = last user message
let out = c.compact(&messages, Some("which port does nerve12 use?"));
// out.messages: Vec<OutMessage { src, role, content: Cow<str>, tool_call_id, tool_calls }>
// out.stats: input/output tokens, dup_exact, dup_near, referenced_units, kept_turns, over_budget
let short = openmind_compact::compact_text(doc, query, 2_000); // one document
```

CLI: `openmind-compact compact --budget N [--query Q] [--keep-turns K] < in.jsonl > out.jsonl`
(stats JSON on stderr), `openmind-compact bench [--reps R] [--real FILE]...`.

## Where it would plug in (not done; needs Slavik's go)

1. **hermes-rs `agent.rs` `fit_context()`**: when `sum(estimate) > context_length - reserve`,
   map `turn.messages` (serde `Value`) to `Message` (content string, `tool_call_id`,
   `tool_calls[].id`), call `Compactor::compact(&msgs, Some(user_text))`, and map back.
   Keep the current front-trim as the fallback. Cost: ~7 ms per 100k tokens, once per model
   call. One `Compactor` per agent thread (it is `!Sync` scratch).
2. **openmind-memory `POST /v2/offload/compact`** (501 today): body `{session_id, messages,
   budget_tokens, query?}` → compacted messages + stats; the same crate with no new dependencies.
3. **openmind-memory `read-ref`**: replace `truncate_prefix()` (prefix cut) with
   `compact_text(doc, query, max_tokens)` when a query is known.
4. **hermes-rs session store**: optionally persist the compaction (`compacted = 1`,
   `active = 0` for elided rows), which the SQLite schema already has columns for.

## Benchmark (2026-10-21 17:38–17:40 +02:00, box load average 15–18)

`openmind-compact bench --reps 25`, release with `--config profile.release.opt-level=3`
(the workspace ships opt-level "z"; see the second table). Retention = exact-match:
the answer and its subject are on one line of the compacted text. Query-aware = the
question is the compaction query. Agnostic = the query is the transcript's last user
message (unrelated). Front-trim = hermes-rs `fit_context` at the same budget.
Synthetic facts are planted once each, never in the last two turns. Real logs (box,
non-secret): a snapshot of `/workspace/swarm/CALL-RECORD.md` (17:27 +02:00), the nerve
capture `journal_30m.jsonl` (no facts extracted; latency only) and a cargo test log.

## synthetic agent transcripts (40 planted facts each, none in the last 2 turns)

| case | in tok | budget | ratio | ms med | ms min | ms/100k | ms per query-aware call | dup exact/near | retention agnostic | retention query-aware | front-trim baseline |
|---|---|---|---|---|---|---|---|---|---|---|---|
| synth 10k s1 /4 | 12020 | 2500 | 4.9x | 0.87 | 0.79 | 7.22 | 2.50 | 41/272 | 27.5% (11/40) | 100.0% (40/40) | 0.0% (0/40) |
| synth 10k s2 /10 | 10783 | 1000 | 11.5x | 0.87 | 0.70 | 8.05 | 2.50 | 24/387 | 40.0% (16/40) | 100.0% (40/40) | 0.0% (0/40) |
| synth 10k s3 /20 | 10603 | 500 | 18.5x | 0.87 | 0.82 | 8.16 | 1.38 | 23/385 | 0.0% (0/40) | 0.0% (0/40) | 0.0% (0/40) | OVER
| synth 50k s1 /4 | 51693 | 12500 | 4.6x | 4.16 | 3.67 | 8.04 | 6.00 | 210/1965 | 100.0% (40/40) | 100.0% (40/40) | 20.0% (8/40) |
| synth 50k s2 /10 | 50687 | 5000 | 10.7x | 7.74 | 3.54 | 15.28 | 6.73 | 251/1619 | 97.5% (39/40) | 100.0% (40/40) | 2.5% (1/40) |
| synth 50k s3 /20 | 50827 | 2500 | 21.0x | 3.72 | 3.38 | 7.32 | 9.60 | 309/1717 | 42.5% (17/40) | 100.0% (40/40) | 7.5% (3/40) |
| synth 100k s1 /4 | 101397 | 25000 | 4.5x | 8.09 | 7.02 | 7.98 | 21.78 | 610/3344 | 100.0% (40/40) | 100.0% (40/40) | 17.5% (7/40) |
| synth 100k s2 /10 | 100918 | 10000 | 10.8x | 29.04 | 9.93 | 28.78 | 21.36 | 691/3437 | 100.0% (40/40) | 100.0% (40/40) | 5.0% (2/40) |
| synth 100k s3 /20 | 100934 | 5000 | 21.7x | 23.77 | 11.69 | 23.55 | 20.48 | 567/3473 | 100.0% (40/40) | 100.0% (40/40) | 0.0% (0/40) |
| synth 200k s1 /4 | 200719 | 50000 | 4.5x | 38.62 | 18.67 | 19.24 | 42.77 | 1495/6860 | 100.0% (40/40) | 100.0% (40/40) | 27.5% (11/40) |
| synth 200k s2 /10 | 202259 | 20000 | 10.8x | 15.74 | 13.96 | 7.78 | 36.70 | 1361/7112 | 100.0% (40/40) | 100.0% (40/40) | 5.0% (2/40) |
| synth 200k s3 /20 | 201221 | 10000 | 21.5x | 30.14 | 13.52 | 14.98 | 54.62 | 1517/6753 | 100.0% (40/40) | 100.0% (40/40) | 2.5% (1/40) |

## 100k tokens, 5 seeds per budget (aggregate)

| budget | ratio mean | ms med (median of seeds) | retention agnostic | query-aware | front-trim |
|---|---|---|---|---|---|
| 25000 (4x) | 4.5x | 12.88 | 100.0% (200/200) | 100.0% (200/200) | 19.0% (38/200) |
| 10000 (10x) | 10.8x | 23.13 | 100.0% (200/200) | 100.0% (200/200) | 8.0% (16/200) |
| 5000 (20x) | 21.5x | 17.64 | 83.0% (166/200) | 100.0% (200/200) | 3.0% (6/200) |

## real box logs (cloze facts: hex ID on a line with a rare subject word)

| case | in tok | budget | ratio | ms med | ms min | ms/100k | ms per query-aware call | dup exact/near | retention agnostic | retention query-aware | front-trim baseline |
|---|---|---|---|---|---|---|---|---|---|---|---|
| call-record-snapshot.md /4 | 74401 | 18600 | 4.1x | 7.54 | 6.34 | 10.13 | 13.80 | 266/4 | 22.5% (9/40) | 97.5% (39/40) | 20.0% (8/40) |
| call-record-snapshot.md /10 | 74401 | 7440 | 10.3x | 23.35 | 10.44 | 31.38 | 11.48 | 266/4 | 5.0% (2/40) | 97.5% (39/40) | 7.5% (3/40) |
| call-record-snapshot.md /20 | 74401 | 3720 | 20.6x | 6.65 | 5.81 | 8.94 | 11.67 | 266/4 | 2.5% (1/40) | 92.5% (37/40) | 2.5% (1/40) |
| journal_30m.jsonl /4 | 116618 | 29154 | 4.1x | 20.43 | 9.16 | 17.52 | 0.00 | 74/41 | - | - | - |
| journal_30m.jsonl /10 | 116618 | 11661 | 10.1x | 16.22 | 9.18 | 13.91 | 0.00 | 74/41 | - | - | - |
| journal_30m.jsonl /20 | 116618 | 5830 | 20.5x | 7.59 | 6.49 | 6.50 | 0.00 | 74/41 | - | - | - |
| serde-order-pin-run1-base-p.log /4 | 12061 | 3015 | 4.4x | 0.70 | 0.62 | 5.78 | 2.65 | 133/18 | 67.5% (27/40) | 100.0% (40/40) | 12.5% (5/40) |
| serde-order-pin-run1-base-p.log /10 | 12061 | 1206 | 10.3x | 1.42 | 0.63 | 11.76 | 2.58 | 133/18 | 12.5% (5/40) | 100.0% (40/40) | 0.0% (0/40) |
| serde-order-pin-run1-base-p.log /20 | 12061 | 603 | 22.2x | 0.78 | 0.73 | 6.45 | 0.75 | 163/19 | 12.5% (5/40) | 100.0% (40/40) | 0.0% (0/40) |

### Shipping profile (opt-level "z", LTO), same run window

| case | in tok | budget | ratio | ms med | ms min | ms/100k | ms per query-aware call | dup exact/near | retention agnostic | retention query-aware | front-trim baseline |
| synth 100k s1 /4 | 101397 | 25000 | 4.5x | 122.70 | 15.66 | 121.01 | 27.84 | 610/3344 | 100.0% (40/40) | 100.0% (40/40) | 17.5% (7/40) |
| synth 100k s2 /10 | 100918 | 10000 | 10.8x | 37.32 | 21.74 | 36.98 | 18.40 | 691/3437 | 100.0% (40/40) | 100.0% (40/40) | 5.0% (2/40) |
| synth 100k s3 /20 | 100934 | 5000 | 21.7x | 11.70 | 10.19 | 11.59 | 27.31 | 567/3473 | 100.0% (40/40) | 100.0% (40/40) | 0.0% (0/40) |
| budget | ratio mean | ms med (median of seeds) | retention agnostic | query-aware | front-trim |
| 25000 (4x) | 4.5x | 21.47 | 100.0% (200/200) | 100.0% (200/200) | 19.0% (38/200) |
| 10000 (10x) | 10.8x | 25.32 | 100.0% (200/200) | 100.0% (200/200) | 8.0% (16/200) |
| 5000 (20x) | 21.5x | 31.68 | 83.0% (166/200) | 100.0% (200/200) | 3.0% (6/200) |
| case | in tok | budget | ratio | ms med | ms min | ms/100k | ms per query-aware call | dup exact/near | retention agnostic | retention query-aware | front-trim baseline |
| call-record-snapshot.md /4 | 74401 | 18600 | 4.1x | 15.66 | 9.79 | 21.05 | 29.96 | 266/4 | 22.5% (9/40) | 97.5% (39/40) | 20.0% (8/40) |
| call-record-snapshot.md /10 | 74401 | 7440 | 10.3x | 44.76 | 23.94 | 60.15 | 44.49 | 266/4 | 5.0% (2/40) | 97.5% (39/40) | 7.5% (3/40) |
| call-record-snapshot.md /20 | 74401 | 3720 | 20.6x | 36.91 | 27.29 | 49.61 | 32.80 | 266/4 | 2.5% (1/40) | 92.5% (37/40) | 2.5% (1/40) |

Latency under load is noisy: the box runs ~20 agents (load average 11–18), so **min** is
the cleanest figure. In a quieter run at 17:24 +02:00 (load ~12, O3), 100k tokens took
6.6–7.8 ms median.

### Reading the numbers

* Compaction is **~7 ms per 100k tokens** (O3) and ~10–22 ms with the size-optimised
  shipping profile: negligible next to a model call.
* **Query-aware is the strong case** (100 % synthetic, 92.5–97.5 % real). If the current
  question names the entity, the line that answers it survives even at 20x.
* **Query-agnostic on real logs is weak** (2.5–22.5 %, about the same as front-trim). Without
  a question, the algorithm cannot know which of hundreds of SHA-bearing lines will matter.
  This is the honest limit of model-free extraction. Mitigations: keep the full transcript
  in openmind-memory and compact per question (query-aware), or add a model stage later.
* At 10k tokens with a 500-token budget, the system prompt alone (~700 tokens) is over budget;
  the result is flagged `OVER` and nothing is retained. That is expected.
