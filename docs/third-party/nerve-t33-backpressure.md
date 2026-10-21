<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: nerve-t33-backpressure (wave-4 slot 187)

No new dependencies and no copied code. The nerve-client change is std only
(`std::net::TcpStream` timeouts). The real test `crates/openmind-nerve/tests/real_backpressure.rs`
uses only crates already in openmind-nerve's `[dependencies]`/`[dev-dependencies]`
(serde_json MIT OR Apache-2.0, zagreb-time workspace MIT). Cargo.lock unchanged.
