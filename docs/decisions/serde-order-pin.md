<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Decision: pin serde_json `preserve_order` once for the workspace

Slot 146 `serde-order-pin`, branch `swarm/serde-order-pin`, base
`swarm/integration-2` df42e0f. Written 2026-10-21 07:05 CEST (+02:00).
Status: **patch ready, owner decides (D-SOP1, D-SOP2 below).**

## The problem in one paragraph

Five member crates switched `preserve_order` on for themselves
(`desk-hermes`, `hermes-rs`, `link-watch`, `openmind-memory` as normal deps,
`galion-desk` as a dev-dep). Cargo unifies features over every package in one
build. So the same source gives two different JSON key orders:

| Build command | `serde_json::Value` / `json!` key order in 18 other crates |
|---|---|
| `cargo build -p openmind-nerve` (how deploys and `scripts/real/*` build) | **sorted** (BTreeMap) |
| `cargo build --workspace` (how `.github/workflows/release.yml` builds) | **insertion order** (IndexMap) |
| `cargo test -p one-set-model -p link-watch` | insertion order, because link-watch is in the command |

Structs with `#[derive(Serialize)]` are not affected (field order always);
only `Value`, `Map` and `json!` output is.

## Evidence (box + VPS, all times CEST +02:00)

- `cargo tree -e features -i serde_json`, per member, on df42e0f: 6 crates ON
  with `-p` alone (7 counting dev-deps), **18 crates off**: gaia, gaia-live,
  gaia-poseidon, galion-boot, galion-realtest, guardian-tick, hades-gate,
  one-defence, one-set-model, one-talk, openmind-discovery, openmind-exo,
  openmind-llama, openmind-mcp-stack, openmind-nerve, openmind-provider,
  openmind-queue, openmind-schema, openmind-speech. With `--workspace`: all ON.
- Same test, different verdict (07:00): new `one-set-model/tests/key_order.rs`
  on the unpatched tree **FAILS** with `-p one-set-model` (keys come back
  `alpha, mid, zeta`) and **PASSES** with `-p one-set-model -p link-watch`.
  With the pin it passes with `-p one-set-model` alone.
- Same nerve source, two binaries, side by side on the box (07:02:24):
  - unpatched `-p openmind-nerve` on :20161 →
    `{"appended":0,"cycle":null,"score":null,"service":"openmind-nerve","status":"ok",...}`
  - pinned `-p openmind-nerve` on :20162 →
    `{"status":"ok","service":"openmind-nerve","version":"0.1.0","appended":0,...}`
    (`/api/v1/state` starts `tool, service, version, ts, started, cycle, ...`
    instead of `alerts, components, ...`).
- The live VPS already runs **both** orders (read-only GETs, 07:02:34):
  - galion-nerve :8670 `/state` and galion-nerve-shadow :8672 `/health` and
    `/api/v1/state`: **sorted** (`{"appended":0,"cycle":291,"score":70,...}`),
    i.e. they were built with `-p`.
  - hermes-rs :8642 `/health`: `{"status":"ok","platform":"hermes-agent",
    "version":"0.21.4",...}` and openmind-memory :8420 `/health`
    `{"status":"ok","version":"0.1.0","uptime":...}`: **insertion order**, the
    Python-compat order these crates were written for.
- Earlier victim: `docs/real-tests/nerve.md` §5, the nerve `/event` reply test
  passed with `-p` and failed under `cargo test --workspace`; it was worked
  around with a fixed string instead of fixing the cause.

## Options

| | Option | -p == --workspace | Python-compat crates | Changes live output? | Cost |
|---|---|---|---|---|---|
| **A (recommended)** | Pin `preserve_order` ON in root `[workspace.dependencies]`; members `{ workspace = true }` only; repo-check `deps` enforces | yes | kept | 18 off-crates switch sorted → source order on their next `-p` rebuild (nerve, shadow, gaia, hades-gate, guardian-tick, ...) | +2 to +11 KB per binary (indexmap+hashbrown+equivalent, all MIT OR Apache-2.0, already in Cargo.lock) |
| B | Pin OFF everywhere (drop the 5 per-crate features) | yes | **broken**: hermes-rs, memory, desk-hermes, link-watch lose the Python key order (`docs/ucpu.md`: "part of the Python-compat contract"); hermes/memory goldens fail | hermes-rs, memory, link-watch, desk change on next rebuild | none |
| C | Keep as is, build releases with `-p` only and add order-agnostic tests | no | kept | none now | the trap stays: any new `--workspace` build or new crate silently flips order again |
| D | Each of the 18 crates adds `features = ["preserve_order"]` itself | yes today, not for a 19th crate | kept | same as A | same as A, 18 edits and no single source of truth |

### Why A

1. One answer for every build path: `-p`, `--workspace`, CI release, `cargo test`.
2. It matches what the Python side does (`json.dumps` keeps insertion order),
   and what the already-Python-compat crates need; one-set-model's `render`
   now keeps a config file's own key order instead of sorting the file.
3. What `cargo test --workspace` (the CI gate) already tested is now exactly
   what ships; before the pin the 18 off-crates shipped an untested variant.
4. Cheap: no new crate in Cargo.lock; +5 KB on the nerve (1 007 816 →
   1 012 872 B, host release, `opt-level=z`, LTO).

### Risks of A, and how they are covered

- **Clients that compare raw JSON text** of the 18 off-crates (byte diffs,
  hashes over `to_string()` of a `Value`, string-equal asserts). Inside this
  repo the whole test suite passes with the pin, both `-p` per crate and
  `--workspace` (see docs/real-tests/serde-order-pin.md). Readers outside the
  repo were not audited. Owner check: D-SOP2.
- **Nerve and shadow must be rebuilt together**: if only one of
  galion-nerve / galion-nerve-shadow is rebuilt from a pinned tree, a byte
  diff between them would show every key moved. Parsed comparison is unaffected.
- **Signed data**: hades-gate tokens sign a canonical ASCII line, not JSON
  (`crates/hades-gate/src/token.rs`), so signatures do not change.
  openmind-memory hashes `json!(...).to_string()` for node ids, but memory
  was already ON, so its ids do not change.

## The patch (this branch)

- `Cargo.toml`: `serde_json = { version = "1", features = ["preserve_order"] }`
  in `[workspace.dependencies]`, with a comment pointing here.
- `crates/{desk-hermes,galion-desk,hermes-rs,link-watch,openmind-memory}/Cargo.toml`:
  `features = ["preserve_order"]` removed (now inherited).
- `crates/repo-check`: new `deps` check (error): root must pin
  `preserve_order`; `crates/*/Cargo.toml` may only write
  `serde_json = { workspace = true }` (optional/comments allowed, no
  `features`, no own version, no `[dependencies.serde_json]` table).
  `edge/openmind-node` (own workspace) and `contrib/` are out of scope.
- `crates/repo-check/tests/deps_pin.rs`: 12 unit tests incl. a scan of this repo.
- `crates/repo-check/tests/real_serde_order.rs` (`GALION_REAL=1`): asks cargo
  for every member with `-p` and `--workspace`, then builds
  `-p openmind-nerve` release alone, serves it on :20160 and checks the raw
  `/health` key order.
- `crates/one-set-model/tests/key_order.rs` + doc comment of `render` fixed
  (it said "sorted").
- Cargo.lock: unchanged.

## Owner decisions

- **D-SOP1** Approve option A (pin ON) and merge `swarm/serde-order-pin` into
  the next integration branch? Alternatives B/C/D above. Recommendation: A.
- **D-SOP2** After merge, the next rebuild of galion-nerve and
  galion-nerve-shadow changes their JSON key order (same values). Confirm that
  no outside reader of :8670 `/state` or :8672 compares raw text, and rebuild
  both binaries in the same deploy. Not urgent: nothing changes on the VPS
  until someone deploys a new nerve build.

Not in scope: `edge/openmind-node` (separate workspace, serde_json without
`preserve_order`, consistent with itself) and `contrib/grokbot/mcp/vps-mcp`
(vendored, excluded).

## Merge note for later integration branches

`integration-3` 625eaf0 and `integration-4` 42de672 carry three more crates
with their own `features = ["preserve_order"]` (`bench-banking`,
`galion-bench`, `hermes-bench`). When this branch is merged there, drop those
three `features` too; repo-check `deps` names each line, so CI points at them.
