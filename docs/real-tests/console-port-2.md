<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: console-port-2 rooms against the live VPS (read-only)

Slot 136 ports 20060-20069 on the box. `crates/console-static/tests/real_port2.rs` (`#[ignore]` + `GALION_REAL=1`):
opens `ssh -N` to the VPS (alias `slavik-vps`, address not in the code) with forwards 20061 -> 127.0.0.1:8080 (console API),
20062 -> 8646 (galion-desk), 20063 -> 8670 (galion-nerve), starts the real release `galion-console-static --ui rust
--upstream 127.0.0.1:20061 --desk 127.0.0.1:20062 --bind 127.0.0.1:20060`, compares every room with the live API read
through the same forward, then stops the binary and the forward. **Only GETs reach the VPS.** Scan, Heal, Boot and Talk are
tested against a fake upstream only (`tests/ui_port2.rs`). Nothing was started, written or changed on the VPS.

```
GALION_REAL=1 cargo test --release -p console-static --test real_port2 -- --ignored --nocapture
```

## Final run, 2026-10-21 05:26:22-05:27:00 CEST (+02:00), branch code as committed

```
real: forward 127.0.0.1:20061->8080 127.0.0.1:20062->8646 127.0.0.1:20063->8670 (ssh -N, GET only)
real: nerve before: cycle 6583 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: /context 200 21071 B in 846 ms
real: /context ok: ctx 65536 window 262144 road 12 tools 12
real: /gateway ep=0 GET /api/v1/health -> live 200, page 17014 B in 798 ms
real: /gateway ep=1 GET /api/v1/domains -> live 200, page 19954 B in 797 ms
real: /gateway ep=2 GET /api/v1/services -> live 200, page 24180 B in 798 ms
real: /gateway ep=3 GET /api/v1/repos -> live 200, page 23175 B in 799 ms
real: /gateway ep=4 GET /api/v1/markers -> live 200, page 15839 B in 611 ms
real: /gateway ep=5 GET /api/v1/ping -> live 200, page 22685 B in 1025 ms
real: /gateway ep=6 GET /api/v1/ledger -> live 200, page 46894 B in 804 ms
real: /gateway ep=8 GET /api/v1/catalog -> live 200, page 26678 B in 800 ms
real: /gateway ep=9 GET /api/v1/protocol/snapshot -> live 200, page 33911 B in 800 ms
real: /guardian live status 401, page says so
real: /hermes 200 28077 B in 807 ms
real: /hermes threads 214 (listed 60), thread items 18
real: nerve after: cycle 6584 mode live components 15 -> ["hermes-rs:8642 healthy", "galion-desk:8646 healthy", "galion-one:8091 healthy", "console:8080 healthy"]
real: stopped galion-console-static
real: stopped ssh forward
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 29.38s
```

What is checked against live data (not only status codes):
- `/context`: `ctx` and `mindpack.context.model_window` from live `/desk` appear as "65536 live · 262144 window"; the Road has
  exactly road + 2 entries (Now, Cache, 12 sessions); one `<progress>` per live tool (12) and every tool name shown.
- `/gateway`: for each of the 9 GET endpoints the page shows the same HTTP status as a direct GET (all 200). The 2 POST
  endpoints were not sent live.
- `/guardian`: live `GET /api/v3/guardian/status` is `401 {"error":"phone required"}` for an unpaired caller (the TSX shows
  "… · score — · incidents 0" then); the Rust page shows "HTTP 401 (phone required)". A paired run was not possible (no phone
  cookie on the box, on purpose).
- `/hermes`: thread count equals live `/api/hermes/history` items (214), 60 links listed, the first thread's item count (18)
  equals the live `history?id=` items and the page renders 18 entries; the gateway bind address is not in the page.
- No `<script>` in any room; the nerve (VPS 127.0.0.1:8670 `/state`, mode live) saw console, desk, hermes-rs and galion-one
  healthy before and after. The cortex is not live: galion-nerve-shadow (8672, observe-only, cycle 86 at ~05:21) answers
  `/healthz` 200 with cortex score `null`.
- After the run: no listener left on 20060-20069; VPS `console.service` NRestarts=0, 0 error/fail journal lines since 05:18.

Earlier runs: 05:19:15-05:19:52 and 05:25:18-05:25:54 (+02:00), same results; the `/gateway` pages took 1.4-2.1 s there
because the header-dot health GET ran after the send. It now runs in parallel: 0.6-1.0 s (each upstream call crosses the
ssh forward; on the VPS itself these are loopback calls).

## Live API shapes used for the fakes (read 2026-10-21 05:09 CEST, values not committed)
`/desk` keys: at, board, ctx, events, focus, foundation, heard, hermes, hold, line, mindpack{cache, cached, context, history,
mind, models, recent, road, roles, sessions, stores, tasks, tools}, model, notes, one, resting, said, services, tunnel, turns,
voice, vram, waking. `/api/hermes/history`: {ok, items[{id, title, count, at, source, pinned, input_tokens, output_tokens,
cache_read_tokens, cache_write_tokens, tokens{input, output, total}}], files}. `history?id=`: {ok, tokens, items[{kind, text} |
{kind:"tool", name, status, command, output}]}.

## Box checks (05:20-05:27 +02:00)
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect
  -- -D warnings` ok; `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 553 passed, 0 failed,
  2 ignored (125 suites).
- console-ui lib 40 tests, console-static: ui.rs 11, ui_port2.rs 8, unit 23, serve 6.
- `galion-console-static` release: x86_64 musl 795 424 B (static-pie), aarch64 musl 684 712 B (rust-lld); armv7 musleabihf and
  aarch64-linux-android `cargo check` ok. `scripts/no-python.sh` ok. repo-check: 783 files, 0 errors, 43 warnings (all
  pre-existing) -> PASS. No new crates (`Cargo.lock` unchanged).
