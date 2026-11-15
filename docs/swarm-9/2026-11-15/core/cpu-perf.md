# Swarm 9 core / cpu-perf

Written 2026-11-15T15:37:52+01:00 (Europe/Zagreb). Lane cpu-perf. Repo `obsidian-open-mind`, branch off `main`.

The open draft titled "Swarm 9 core plan" was not on GitHub when this lane started (search of open pull requests, 2026-11-15). Work stayed on `main` and did not touch other draft branches.

`LICENSE`, `README`, `STATUS`, and the dependency manifests were not edited. No new crate was added.

## Machine

`/proc/cpuinfo` model name: `Intel(R) Xeon(R) Processor`. Family 6, model 207, stepping 2. 4 CPUs. The line is the hypervisor brand string. A more specific SKU was not printed.

`rustc 1.100.0 (0ef1bc386 2026-11-09)`.

`perf` is not installed. `perf` was not run.

Timings use Cargo's bench profile (`Finished bench profile [optimized]`, opt-level 3). The workspace release profile is opt-level `z` with lto and codegen-units 1. Release-profile timings: not measured.

`cargo bench` on stable ignores `#[test]` and reported the bench as ignored. The timed command is `cargo test --profile bench --bench cpu`, which runs the same bench-profile binary.

Each figure is the median of 5 samples, nanoseconds per call. The before run is the first full run, before the edits that stayed. The after run is the final tree, started 2026-11-15T15:37:08+01:00.

## Commands

Before:

```
cargo test -p openmind-desktop --profile bench --bench cpu --locked -- --nocapture
cargo test -p hermes-rs --profile bench --bench cpu --locked -- --nocapture
```

`request_hdr_value_plain` was not in that first Hermes run. It was measured with the same Hermes command after the case was added and before `hdr_value` changed: median 188 ns, samples 186, 186, 188, 192, 192.

After (final tree, same two commands): medians below.

## What the benches call

Desktop (`crates/openmind-desktop/benches/cpu.rs`): `parse_head` plus `reply` for `/`, `/files`, `POST /files`, and a 404. No `GET /probes` (that dials). File gate: `Files::read_path` on a closed gate (path parse, no disk), `classify`, `sanitize_name`, `json_path`. Probes: `ProbeConfig::parse`, `ProbeSet::to_json`, `observed_mode`, and `probe_one` on a disabled endpoint (no socket).

Hermes (`crates/hermes-rs/benches/cpu.rs`): `parse_head`, `Request::from_common`, header lookup, `json`, `full_bytes`, `hdr_value`. MCP on main: `sanitize`, `prefixed_name`, `parse_line` (the stdio reader), `tool_result_text` (the `tools/call` text join, before redaction).

## Kept

Median ns/op, before then after.

| bench | before | after | change |
| --- | ---: | ---: | --- |
| file_gate_json_path_2kib | 17895 | 1696 | ASCII runs copied in one slice |
| file_gate_json_path | 367 | 171 | same |
| request_hdr_value_plain | 188 | 25 | ASCII header value with no whitespace, sliced |
| mcp_prefixed_name_long | 2426 | 995 | 4 digest bytes via `galion_common::hex::encode` |
| mcp_prefixed_name | 273 | 163 | one string instead of `format!` |
| request_from_common_header_json | 1628 | 951 | in-place ASCII lower, no alloc when the lookup name is already lower |
| request_reply_upload | 1570 | 1007 | JSON path plus `Response::to_bytes` |
| request_reply_files | 983 | 618 | `Response::to_bytes` writes headers into one string |
| request_reply_404 | 953 | 632 | same |
| file_gate_sanitize_long | 2209 | 1721 | character count, no second string when trim does not shorten |
| file_gate_sanitize_name | 194 | 147 | same |
| mcp_sanitize | 96 | 67 | reserve capacity |
| file_gate_classify_small | 74 | 63 | allow-list match, no lowercased `String` |

`Response::to_bytes` still drops CR, LF, and NUL headers, and still refuses to copy `content-length` and `connection` from the caller. The existing `response_bytes` test checks the wire text.

`prefixed_name` still clamps at 64 with `_` plus the first 8 hex digits. The unit test checks those 8 digits against `sha256`.

## Measured, not changed

Later runs of untouched code moved by roughly the same amount as some of the smaller reply numbers, so those are not claimed as optimizations.

| bench | before | after | note |
| --- | ---: | ---: | --- |
| request_parse_head (desktop) | 693 | 602 | `parse_head` not edited |
| request_parse_head_24 | 3983 | 3833 | not edited |
| request_reply_root | 3703 | 3119 | page copy dominates; drop is close to the untouched `parse_head` movement |
| request_parse_head (hermes) | 1164 | 891 | not edited; first sample spread was 1015 to 1404 |
| request_full_bytes | 81 | 75 | not edited |
| request_hdr_value | 153 | 141 | whitespace path still split/join/filter. A second before run, before the plain fast path, was 132 |
| request_hdr_value_ws | 117 | 116 | not edited. A second before run was 105 |
| file_gate_parse_path | 131 | 119 | not edited |
| file_gate_parse_traversal | 45 | 43 | not edited |
| file_gate_classify_32kib | 2034 | 2481 | std UTF-8 check left as it is. This after sample spread was 2119 to 3350 |
| file_gate_classify_png | 106 | 108 | magic check dominates |
| probe_parse_spec | 2062 | 2159 | not edited in the final tree |
| probe_to_json | 1510 | 1670 | not edited in the final tree |
| probe_observed_mode | 418 | 429 | function made public, body unchanged |
| probe_one_disabled | 37 | 39 | not edited |
| mcp_parse_tools_list | 2574 | 2574 | `serde_json`, not replaced |
| mcp_parse_tools_call | 1488 | 1445 | same |
| mcp_tool_result_text | 313 | 285 | same algorithm, moved into `tool_result_text` |
| mcp_parse_skip | 30 | 32 | not edited |

## Tried and not kept

- Probe JSON integers written from a stack buffer: that run went from 1510 ns to 1709 ns. Reverted.
- `tool_result_text` built in one string: 313 ns to 318 ns. Reverted to the previous join.
- `ProbeConfig::parse` skipping default endpoint strings: a repeat did not stay faster. Reverted.

## Tests

`cargo test -p galion-common -p hermes-rs -p openmind-desktop --locked --lib --tests` on 2026-11-15:

- galion-common: 23 lib + 4 `http_evict` + 6 `http_fuzz` + 3 `nist` + 1 `real_common` (1 ignored) + 1 `real_hammer` + 1 `real_migrate` (1 ignored) + 1 `real_miri_capture` + 1 `real_version` + 3 `time_vs_chrono`. 0 failed.
- hermes-rs: 49 lib + 2 `enroll_http` + 16 `golden` + 1 `heap_growth` + 1 `mcp`. 0 failed.
- openmind-desktop: 43 lib. 0 failed.

`cargo fmt -p openmind-desktop -p hermes-rs -p galion-common -- --check` passed.
`cargo clippy -p openmind-desktop -p hermes-rs -p galion-common --all-targets --locked -- -D warnings` passed.

`cargo test --workspace` and `cargo clippy --workspace`: not run.

## Open questions for Maciej

- These benches are compiled by `cargo clippy --all-targets`. They are not run by `cargo test --workspace`. Whether CI should time them is open.
- The numbers are opt-level 3. The release binaries are opt-level `z`. How much of the gain remains there: not measured.
- `observed_mode`, `parse_line`, and `tool_result_text` are now public so the benches call the functions the console and the MCP client use.
