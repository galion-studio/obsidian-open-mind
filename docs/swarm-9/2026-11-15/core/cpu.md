<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Maciej Grajczyk -->

# Swarm 9 core/cpu

Lane notes for the CPU pull request. Written 2026-11-15T15:44:49+01:00, desktop section added 2026-11-15T15:47:45+01:00 (Europe/Zagreb). Base is `main`.

The plan at `dda1c88b` and the desktop plan at `c8439c04` keep `crates/openmind-desktop/src/files.rs`, `src/probe.rs`, and `benches/cpu.rs` on this pull request. `crates/hermes-rs/**` stays with desktop until the #14 recreation and #70 have each merged, so that whole tree is absent here. `crates/galion-common/src/http.rs` is hygiene's. It is absent here. `git diff main` for those two paths is empty.

## Machine

`/proc/cpuinfo` model name is `Intel(R) Xeon(R) Processor`, vendor GenuineIntel, family 6, model 207, stepping 2, 4 CPUs. The flags include `hypervisor`. No SKU beyond that string. `nproc` is 4. `RUSTFLAGS` was unset. `rustc 1.100.0 (0ef1bc386 2026-11-09)`. Bench profile is the cargo default (opt-level 3). Release-profile timings (`opt-level = "z"`): not measured. `perf`: not run.

## What was timed

`galion-jsonout`: `to_json_compat` (that is `PyFormatter`) on one object with ASCII keys and both ASCII and non-ASCII string values, compact and spaced.

`galion-bench`: `read_request` on a small GET, `write_response` of a small JSON body, and a real hit of `MockServer` bound with `Ipv4Addr::LOCALHOST` and port 0. The fixture answers `GET /health`. No bank, broker, or supplier call.

`openmind-llama`: `parse_health` on `fixtures/health_ok.json` and `parse_models` on `fixtures/v1_models.json`. No socket and no GPU.

SHA-256 stays in `galion-common`. `galion-bench` does not depend on that crate. Calling it from this bench would add a dependency and a lockfile edge. Not added. SHA-256 timing: not measured.

## Command

Same command before the two source edits and twice after. Exit status 0 each time.

```
cargo test -p galion-bench -p galion-jsonout -p openmind-llama --profile bench --bench cpu --locked -- --nocapture --test-threads=1
```

Each figure is the median of 5 samples, nanoseconds per call, after 8 warmup calls. `std::time::Instant`. The clock read 2026-11-15T15:44:49+01:00 immediately after the second after-run. The before-run and the first after-run were the two commands before that, in this same session.

| path | iters | before | after (run 1) | after (run 2) | kept |
| --- | --- | --- | --- | --- | --- |
| `response_write` | 20000 | 335 (332–344) | 211 (207–222) | 220 (214–227) | yes |
| `py_formatter_compact` | 20000 | 602 (579–682) | 510 (507–530) | 525 (524–547) | yes |
| `py_formatter_spaced` | 20000 | 602 (577–636) | 506 (499–548) | 513 (510–521) | yes |
| `request_read` | 20000 | 702 (688–703) | 691 (690–702) | 699 (693–702) | no edit |
| `loopback_mock_hit` | 200 | 70130 (63586–84977) | 68611 (63407–83009) | 68826 (62531–78592) | no claim |
| `parse_health` | 40000 | 202 (198–205) | 201 (198–202) | 205 (203–207) | no edit |
| `parse_models` | 10000 | 6578 (6519–6779) | 6606 (6537–6656) | 6795 (6565–6893) | no edit |

The loopback hit is a thread plus a TCP round trip. Its sample spread is larger than the `write_response` change, so that hit is not claimed as faster.

## Edits that stayed

`write_response` builds one `String` and writes the status and the content length without `format!`. Header names `content-length` and `connection` are skipped with `eq_ignore_ascii_case`. The bytes are the same shape the existing round-trip test checks.

`PyFormatter::write_string_fragment` writes an ASCII fragment in one `write_all`. A fragment with a non-ASCII scalar still uses the existing escape loop. `tests::cpython_bytes` covers the café and emoji cases.

No SIMD, no `target-cpu`, no `target-feature`, no new dependency, no manifest edit.

## Desktop file gate

`crates/openmind-desktop/src/files.rs` copies an ASCII JSON-string run in one slice, counts characters while sanitising a name, and matches the allow-list extension without a lowercased `String`. `probe.rs` only makes `observed_mode` public so the bench can call it. The function body is unchanged. `galion-common` `Response::to_bytes` is the stock function.

Before numbers are the first desktop run on this machine, 2026-11-15T15:37:08+01:00, before those `files.rs` edits. Command:

```
cargo test -p openmind-desktop --profile bench --bench cpu --locked -- --nocapture
```

Exit status 0. This tree, same command, finished just before 2026-11-15T15:47:45+01:00. Exit status 0. Median of 5 samples, nanoseconds per call, after 32 warmup calls.

| path | iters | before | this tree | kept |
| --- | --- | --- | --- | --- |
| `file_gate_json_path_2kib` | 20000 | 17895 | 1733 (1715–1792) | yes |
| `file_gate_json_path` | 100000 | 367 | 192 (187–202) | yes |
| `file_gate_sanitize_long` | 40000 | 2209 | 1565 (1557–1613) | yes |
| `file_gate_sanitize_name` | 100000 | 194 | 156 (149–164) | yes |
| `file_gate_classify_small` | 100000 | 74 | 63 (63–65) | yes |
| `request_reply_upload` | 40000 | 1570 | 1368 (1359–1383) | no |
| `request_reply_files` | 40000 | 983 | 983 (978–987) | no |
| `request_reply_404` | 40000 | 953 | 937 (936–953) | no |
| `request_reply_root` | 4000 | 3703 | 3787 (3736–3795) | no |
| `request_parse_head` | 100000 | 693 | 591 (586–598) | no edit |
| `request_parse_head_24` | 40000 | 3983 | 3825 (3808–3900) | no edit |
| `file_gate_parse_path` | 100000 | 131 | 113 (111–115) | no edit |
| `file_gate_parse_traversal` | 100000 | 45 | 45 (42–45) | no edit |
| `file_gate_classify_32kib` | 2000 | 2034 | 1939 (1871–1943) | no edit |
| `file_gate_classify_png` | 100000 | 106 | 104 (103–113) | no edit |
| `probe_parse_spec` | 40000 | 2062 | 2142 (2128–2255) | no edit |
| `probe_to_json` | 40000 | 1510 | 1504 (1486–1537) | no edit |
| `probe_observed_mode` | 40000 | 418 | 535 (531–562) | no |
| `probe_one_disabled` | 100000 | 37 | 39 (36–43) | no edit |

The earlier reply figures that depended on a `to_bytes` edit are not this tree. `request_reply_files` is back at the before median. `probe_observed_mode` is slower on this run. The body was not changed.

## Tests

```
cargo fmt -p galion-bench -p galion-jsonout -p openmind-llama -- --check
cargo test -p galion-bench -p galion-jsonout -p openmind-llama --locked --lib --tests
cargo clippy -p galion-bench -p galion-jsonout -p openmind-llama --all-targets --locked -- -D warnings
```

Exit status 0 for each. `galion-bench`: 11 lib, 0 in the binary, 2 in `real_mock`, 4 in `server`. `galion-jsonout`: 1 lib. `openmind-llama`: 0 lib, 13 in `parse`.

```
cargo fmt -p openmind-desktop -- --check
cargo test -p openmind-desktop --locked --lib --tests
cargo clippy -p openmind-desktop --all-targets --locked -- -D warnings
```

Exit status 0. `openmind-desktop`: 43 lib, 0 in the binary. `cargo test --workspace` and `cargo clippy --workspace`: not run.
