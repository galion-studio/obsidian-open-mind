<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# nerve-client

Optional, fire-and-forget push of signals to `galion-nerve` (`POST /signal`).
std only, no dependencies, loopback `http://` only.

- `NERVE_URL` unset (default): every call is a no-op.
- `NERVE_URL=http://127.0.0.1:8670`: each signal goes out on a short-lived
  thread with 500 ms timeouts; at most 4 in flight, extras are dropped. The
  caller is never blocked and never sees an error.

```rust
nerve_client::event("hermes-rs", "started", "listening on 127.0.0.1:8642");
nerve_client::error("hermes-rs", "http_5xx", "POST /v1/chat/completions -> 502");
nerve_client::metric("hermes-rs", "queue_depth", 3.0);
nerve_client::start_heartbeat("hermes-rs", 60); // carries VmRSS in KiB
```

Wired on ws/nerve into `hermes-rs` (`api::spawn`: started event, 60 s
heartbeat, every 5xx response) and `openmind-memory` (`main`: same).
