<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: cortex-api-load (slot 236)

No new dependencies and no copied code. Hermes research was not used ($0).

The fix extends the repo's own std-only `galion-common::http` server
(`Limits::evict_idle`) instead of adding a crate: the cortex and ~8 other
services already run on it, and the usual small std servers do not solve
this case either. `tiny_http` (MIT OR Apache-2.0) and `rouille` on top of it
use a thread pool with the same "idle socket holds a worker" property;
`may_minihttp` / `hyper` + `tokio` would bring an async runtime into a
service that is idle-zero by design. Evicting the oldest connection that
has not finished its request is the classic slowloris mitigation (the same
idea as nginx's `reset_timedout_connection` / Apache `mod_reqtimeout`,
re-implemented here in ~60 lines, not copied).
