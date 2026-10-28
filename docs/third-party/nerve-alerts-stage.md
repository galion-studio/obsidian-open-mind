<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-alerts-stage (slot 382)

No new dependencies and no copied code. The webhook client is std only
(`std::net::TcpStream`, hand-written HTTP/1.1 POST) plus the existing serde_json.
Considered: `ureq` (MIT OR Apache-2.0) - rejected, adds rustls/TLS and ~20 crates to a
size-budgeted binary for a loopback-only POST. ntfy (Apache-2.0) is only named as an
optional relay target (owner decision D-382-1); nothing of it is vendored.
