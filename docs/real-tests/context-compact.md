<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: context-compact (openmind-compact)

Box only. No ports, no network, no VPS, nothing live. Real input is the live
`/workspace/swarm/CALL-RECORD.md` swarm log (non-secret by swarm rule; only aggregate
numbers are printed).

Command (2026-10-21 18:16:00–18:17:29 +02:00):

```
GALION_REAL=1 cargo test --release --config 'profile.release.opt-level=3' \
  -p openmind-compact --test real_cli -- --nocapture
```

`real_cli_compacts_box_log_within_budget`: 1 196 JSONL messages (each record entry is a
tool result inside a user/assistant/tool turn), budget 1/10. The release binary printed:

```
{"dup_exact":797,"dup_near":4,"elapsed_us":40573,"input_tokens":80159,"kept_turns":2,
 "messages_in":1196,"messages_out":87,"output_tokens":8705,"over_budget":false,
 "referenced_units":33,"selected_units":83,"units":1681}
```

Checks passed: valid JSONL out, system first, last user message kept, output ≤ budget,
no orphan tool result, < 2 s. The 40.6 ms includes JSON parsing on a loaded box (load
average ~15).

`real_cli_bench_reports_retention` (same run, `bench --reps 3 --real CALL-RECORD.md`):

| case | in tok | budget | ratio | ms med | retention agnostic | query-aware | front-trim |
|---|---|---|---|---|---|---|---|
| CALL-RECORD.md /4 | 76942 | 19235 | 4.1x | 7.07 | 22.5% (9/40) | 97.5% (39/40) | 20.0% (8/40) |
| CALL-RECORD.md /10 | 76942 | 7694 | 10.3x | 7.08 | 10.0% (4/40) | 97.5% (39/40) | 5.0% (2/40) |
| CALL-RECORD.md /20 | 76942 | 3847 | 20.7x | 14.26 | 0.0% (0/40) | 47.5% (19/40) | 2.5% (1/40) |

At 20x on the live log, query-aware retention fell to 47.5 % (the 17:27 snapshot gave
92.5 %). The last two record entries, which are kept verbatim as anchors, are long and
take most of a 3.8k budget. Lowering `keep_last_turns` or capping anchor size is decision
D-CC-3.

Other gates on this branch (2026-10-21 17:46–18:15 +02:00): `cargo fmt --all --check` ok;
`cargo clippy --workspace --all-targets -D warnings` ok; workspace tests (excluding
openmind-sandbox, cgroup-detect, hermes-tools) 1267 passed / 2 failed / 6 ignored.
The 2 failures: `gaia` `ownership_counts`, fixed on the branch (63→64 crates), and the
`openmind-survival` `probe_path` sentinel race under load, which passes 3/3 on rerun and
is not touched by this branch. no-python ok, spdx-scan 0 missing, repo-check PASS
(0 errors, 48 warnings, none in this crate). x86_64 musl static and aarch64 musl static
(with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`; base aa77864 predates
the toolchain-aarch64 linker config) built, and aarch64-android and armv7 checks passed.
