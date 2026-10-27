<!--
SPDX-License-Identifier: MIT
SPDX-FileCopyrightText: 3000 Galion / galion-studio
Copyright (c) 3000 Galion / galion-studio
-->
# Third-party: android-admin-pairing (wave-4 slot 319)

No new dependency and no copied code. The pairing client uses only what the crate already had:

| Crate | Version | Licence | Use |
|---|---|---|---|
| galion-common (in-repo) | workspace | MIT | HTTP/1.x client (`http::send`), SHA-256 (tests) |
| serde_json | workspace pin | MIT OR Apache-2.0 | JSON bodies of `/api/v1/admin` |

Why not reuse a crate: an HTTP client crate (ureq, MIT/Apache) would add TLS/rustls (asm via
ring, against the no-asm rule) or a second HTTP stack; the client deliberately speaks plain
HTTP to loopback only, which `galion-common` already does. A cookie-jar crate (`cookie_store`,
MIT/Apache) is overkill for one HttpOnly cookie; parsing is ~25 lines with unit tests.
