<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Maciej Grajczyk -->

# Swarm 9 core/cpu

Lane notes for the CPU pull request. Written 2026-11-15T15:44:49+01:00 (Europe/Zagreb). Base is `main`.

An earlier commit on this branch timed desktop and Hermes paths and edited `crates/hermes-rs/**`, `crates/galion-common/src/http.rs`, and `crates/openmind-desktop/src/files.rs` plus `probe.rs`. The core plan and the orchestrator review assign those files to other lanes. Those edits and those benches are removed. The numbers from that commit are not this tree and are not repeated here.

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

## Tests

```
cargo fmt -p galion-bench -p galion-jsonout -p openmind-llama -- --check
cargo test -p galion-bench -p galion-jsonout -p openmind-llama --locked --lib --tests
cargo clippy -p galion-bench -p galion-jsonout -p openmind-llama --all-targets --locked -- -D warnings
```

Exit status 0 for each. `galion-bench`: 11 lib, 0 in the binary, 2 in `real_mock`, 4 in `server`. `galion-jsonout`: 1 lib. `openmind-llama`: 0 lib, 13 in `parse`. `cargo test --workspace` and `cargo clippy --workspace`: not run.
