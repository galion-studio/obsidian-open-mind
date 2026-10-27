<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party register: android-admin-plan (wave-4 slot 307)

No new third-party dependency and no copied external code.

| Dependency | Version | Licence (SPDX) | Source | Used for | Why |
|---|---|---|---|---|---|
| `galion-common` (in-repo) | 0.1.0 | MIT | `crates/galion-common` | HTTP/1.1 server + client, SHA-256, base64url, hex, JSON string escaping, Europe/Zagreb RFC 3339 | Already audited std-only plumbing; no new crate needed. |
| `serde_json` (workspace) | 1.x (`preserve_order`, pinned in root Cargo.toml) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | Parse the nerve `/state` and the JSON pair body; build replies | Already a workspace dependency; parsing a 38 KB nerve document by hand is not worth it. |

Design reuse (our own MIT code): the hash-chained audit log follows galion-os `galion-admin`
(`swarm/galion-os-admin-cli` bf33cba, `crates/galion-admin/src/audit.rs`), reimplemented on
`galion-common::sha256`.

Candidates researched for later phases (not added): `qrcode` 0.14.1 (MIT OR Apache-2.0),
`fast_qr` 0.14.0 (MIT), `totp-rs` 6.0.0 (MIT), `adb_client` 3.2.3 (MIT), `web-push` 0.11.0
(Apache-2.0); rejected `sse-codec` 0.3.3 (MPL-2.0) and Termux (GPL-3.0). Details in
`docs/android-admin/PLAN.md` §5.
