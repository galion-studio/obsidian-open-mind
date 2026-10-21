<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: UCPU-1 (slot 13, ports 19130-19139)

Box: x86_64 Debian 13, qemu-user-static 1:10.0.13+ds-0+deb13u1 (installed
2026-10-20 18:51 CEST), rustc 1.99.1, zig 0.15.2. Static aarch64 and armv7
musl release binaries were run explicitly through `qemu-aarch64-static` /
`qemu-arm-static` (no binfmt needed), bound to 127.0.0.1 only, with temp data
dirs. hermes-gateway ran with `--mock` (no provider, no network, no spend);
its `API_SERVER_KEY` was a throwaway test string, not a secret. galion-one
ran with `ONE_NO_WARM=1`. The VPS was not contacted: this test is about the
CPU architecture, and the live VPS services are x86_64, so a VPS comparison
would add nothing here.

Command: `CARGO_TARGET_DIR=/workspace/swarm/target/ucpu ZIG=/workspace/tools/zig-x86_64-linux-0.15.2/zig scripts/real/ucpu.sh`
Result: **20 passed, 0 failed** (2026-10-20 19:01:40 +02:00). All processes
were killed afterwards (`pgrep qemu-*-static`: none; ports 19130-19139 free).

Note: the openmind-memory `/search/conversations` text carries a
UTC (Z-suffixed) timestamp (rewritten below in +02:00 per the Zagreb rule). That is existing behaviour (TencentDB API compat), not
part of this change; flagged for the memory workstream.

Output (cargo lines removed, long bodies cut at 160 chars by the script):

```
[2026-10-20T19:00:49+02:00] zig 0.15.2 at /workspace/tools/zig-x86_64-linux-0.15.2/zig
[2026-10-20T19:00:49+02:00] build galion-one tuner openmind-memory hermes-gateway --target aarch64-unknown-linux-musl
[2026-10-20T19:01:01+02:00] build galion-one tuner openmind-memory hermes-gateway --target armv7-unknown-linux-musleabihf
[2026-10-20T19:01:36+02:00] === aarch64-unknown-linux-musl under qemu-aarch64-static (ports 19130-19134)
    galion-one         732216 bytes  machine=AArch64  NEEDED=0
    tuner              404504 bytes  machine=AArch64  NEEDED=0
    openmind-memory   1619816 bytes  machine=AArch64  NEEDED=0
    hermes-gateway    1750416 bytes  machine=AArch64  NEEDED=0
    listen 127.0.0.1:19130 by qemu-aarch64-st
    listen 127.0.0.1:19131 by qemu-aarch64-st
    listen 127.0.0.1:19134 by qemu-aarch64-st
    listen 127.0.0.1:19132 by qemu-aarch64-st
    listen 127.0.0.1:19133 by qemu-aarch64-st
    http://127.0.0.1:19130/health -> HTTP 200 ok 
  PASS aarch64-unknown-linux-musl galion-one /health
    http://127.0.0.1:19130/me -> HTTP 200 {"you":false}
  PASS aarch64-unknown-linux-musl galion-one /me
    http://127.0.0.1:19132/health -> HTTP 200 {"ok":true} 
  PASS aarch64-unknown-linux-musl tuner /health
    http://127.0.0.1:19132/ -> HTTP 200 <!doctype html><title>tuner ucpu</title> 
  PASS aarch64-unknown-linux-musl tuner /
    http://127.0.0.1:19133/health -> HTTP 200 {"status":"ok","version":"0.1.0","uptime":0,"stores":{"vectorStore":true,"embeddingService":false},"services":{"timerScanner":null,"pipelineWorker":null,"stateB
  PASS aarch64-unknown-linux-musl memory /health
    http://127.0.0.1:19133/capture -> HTTP 200 {"l0_recorded":2,"scheduler_notified":false}
  PASS aarch64-unknown-linux-musl memory /capture (SQLite write)
    http://127.0.0.1:19133/search/conversations -> HTTP 200 {"results":"Found 1 matching message(s):\n\n---\n**[user]** Session: ucpu-aarch64-unknown-linux-musl [service printed UTC; = 2026-10-20T19:01:37.020+02:00] (score: 0.000)\n\nucpu emulated 
  PASS aarch64-unknown-linux-musl memory /search/conversations (SQLite read)
    http://127.0.0.1:19134/health -> HTTP 200 {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}
  PASS aarch64-unknown-linux-musl hermes-gateway /health
    http://127.0.0.1:19134/v1/models -> HTTP 200 {"object":"list","data":[{"id":"hermes-agent","object":"model","created":1792515697,"owned_by":"hermes","permission":[],"root":"hermes-agent","parent":null}]}
  PASS aarch64-unknown-linux-musl hermes-gateway /v1/models
    http://127.0.0.1:19134/v1/chat/completions -> HTTP 200 {"id":"chatcmpl-fe3f27966ab14081","object":"chat.completion","created":1792515697,"model":"mock","choices":[{"index":0,"message":{"role":"assistant","content":"
  PASS aarch64-unknown-linux-musl hermes-gateway /v1/chat/completions (mock, SQLite session)
    pid 757446 galion-one VmRSS     15752 kB
    pid 757447 tuner VmRSS     10640 kB
    pid 757448 openmind-memory VmRSS     19556 kB
    pid 757449 hermes-gateway VmRSS     17364 kB
    log hermes.log: 2026-10-20T19:01:36+02:00 hermes-gateway listening on 127.0.0.1:19134 mock=true loopback=true|
    log mem.log: 2026-10-20T19:01:36+02:00 [openmind-memory] WARN auth disabled (no OPENMIND_MEMORY_API_KEY / TDAI_GATEWAY_API_KEY)|2026-10-20T19:01:36+02:00 [openmind-memory] data_dir=/tmp/ucpu-real.6RovPy/aarch64-unknown-linux-musl/mem db=/tmp/ucpu-real.6RovPy/aarch64-unknown-linux-musl/mem/vectors.db read_only=fa
    log one.log: one: listening|voice: listening|
    log tuner.log: tuner 127.0.0.1:19132 serving /tmp/ucpu-real.6RovPy/aarch64-unknown-linux-musl/www|
[2026-10-20T19:01:38+02:00] === armv7-unknown-linux-musleabihf under qemu-arm-static (ports 19135-19139)
    galion-one         709004 bytes  machine=ARM  NEEDED=0
    tuner              374444 bytes  machine=ARM  NEEDED=0
    openmind-memory   1583556 bytes  machine=ARM  NEEDED=0
    hermes-gateway    1717772 bytes  machine=ARM  NEEDED=0
    listen 127.0.0.1:19138 by qemu-arm-static
    listen 127.0.0.1:19139 by qemu-arm-static
    listen 127.0.0.1:19136 by qemu-arm-static
    listen 127.0.0.1:19137 by qemu-arm-static
    listen 127.0.0.1:19135 by qemu-arm-static
    http://127.0.0.1:19135/health -> HTTP 200 ok 
  PASS armv7-unknown-linux-musleabihf galion-one /health
    http://127.0.0.1:19135/me -> HTTP 200 {"you":false}
  PASS armv7-unknown-linux-musleabihf galion-one /me
    http://127.0.0.1:19137/health -> HTTP 200 {"ok":true} 
  PASS armv7-unknown-linux-musleabihf tuner /health
    http://127.0.0.1:19137/ -> HTTP 200 <!doctype html><title>tuner ucpu</title> 
  PASS armv7-unknown-linux-musleabihf tuner /
    http://127.0.0.1:19138/health -> HTTP 200 {"status":"ok","version":"0.1.0","uptime":0,"stores":{"vectorStore":true,"embeddingService":false},"services":{"timerScanner":null,"pipelineWorker":null,"stateB
  PASS armv7-unknown-linux-musleabihf memory /health
    http://127.0.0.1:19138/capture -> HTTP 200 {"l0_recorded":2,"scheduler_notified":false}
  PASS armv7-unknown-linux-musleabihf memory /capture (SQLite write)
    http://127.0.0.1:19138/search/conversations -> HTTP 200 {"results":"Found 1 matching message(s):\n\n---\n**[user]** Session: ucpu-armv7-unknown-linux-musleabihf [service printed UTC; = 2026-10-20T19:01:38.860+02:00] (score: 0.000)\n\nucpu emula
  PASS armv7-unknown-linux-musleabihf memory /search/conversations (SQLite read)
    http://127.0.0.1:19139/health -> HTTP 200 {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}
  PASS armv7-unknown-linux-musleabihf hermes-gateway /health
    http://127.0.0.1:19139/v1/models -> HTTP 200 {"object":"list","data":[{"id":"hermes-agent","object":"model","created":1792515698,"owned_by":"hermes","permission":[],"root":"hermes-agent","parent":null}]}
  PASS armv7-unknown-linux-musleabihf hermes-gateway /v1/models
    http://127.0.0.1:19139/v1/chat/completions -> HTTP 200 {"id":"chatcmpl-5178542c62475edb","object":"chat.completion","created":1792515698,"model":"mock","choices":[{"index":0,"message":{"role":"assistant","content":"
  PASS armv7-unknown-linux-musleabihf hermes-gateway /v1/chat/completions (mock, SQLite session)
    pid 758807 galion-one VmRSS     13092 kB
    pid 758808 tuner VmRSS      9784 kB
    pid 758809 openmind-memory VmRSS     17816 kB
    pid 758810 hermes-gateway VmRSS     15880 kB
    log hermes.log: 2026-10-20T19:01:38+02:00 hermes-gateway listening on 127.0.0.1:19139 mock=true loopback=true|
    log mem.log: 2026-10-20T19:01:38+02:00 [openmind-memory] WARN auth disabled (no OPENMIND_MEMORY_API_KEY / TDAI_GATEWAY_API_KEY)|2026-10-20T19:01:38+02:00 [openmind-memory] data_dir=/tmp/ucpu-real.6RovPy/armv7-unknown-linux-musleabihf/mem db=/tmp/ucpu-real.6RovPy/armv7-unknown-linux-musleabihf/mem/vectors.db read
    log one.log: voice: listening|one: listening|
    log tuner.log: tuner 127.0.0.1:19137 serving /tmp/ucpu-real.6RovPy/armv7-unknown-linux-musleabihf/www|
[2026-10-20T19:01:40+02:00] result: 20 passed, 0 failed
done 0
```

## Rust real test

`GALION_REAL=1 cargo test -p tuner --test real_ucpu -- --nocapture`
(2026-10-20 19:02 +02:00): the aarch64 `tuner` (ELF e_machine 0xB7) under
qemu-aarch64-static on 127.0.0.1:19132. GET /health → 200 `{"ok":true}`, GET /
→ 200 with the test page, GET /no-such-file → 404.

```
running 1 test
emulated start-up + first reply: 307.623731ms
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
```

VmRSS above is qemu plus the guest, so it overstates native RSS. It is still
under 20 MB per service on both architectures.

> **2026-10-21 (realscript-port-3):** the shell driver for this evidence was ported to Rust and deleted; re-run with the matching test in `crates/galion-realtest/tests/` (see `docs/real-tests/realscript-port-3.md`).
