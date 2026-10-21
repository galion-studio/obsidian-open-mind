<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: `swarm/cortex-s2-c` (T2.8 proc-owner, T2.9 surface)

- Written: 2026-10-20 23:30 CEST (+02:00).
- **No new crate dependency.** `openmind-nerve` stays std + serde + serde_json + zagreb-time; `Cargo.lock` unchanged.
- No TLS stack, no crypto, no openssl: the TLS expiry check builds a TLS 1.2 ClientHello by hand and reads the
  plaintext Certificate message (RFC 5246 §7.4.1.2, §7.4.2; X.509 DER layout RFC 5280 §4.1). Specifications, not code.

| item | licence | source | used for | why not a crate |
|---|---|---|---|---|
| `days_from_civil` algorithm (re-implemented from the description, not copied) | public domain (author's statement) | http://howardhinnant.github.io/date_algorithms.html#days_from_civil | X.509 `notAfter` -> unix seconds in `sensors/surface.rs` | 10 lines; `zagreb-time` formats but does not parse ASN.1 times |
| `tests/fixtures/cortex-s2-c/tls12-flight-galion-app.hex` | public certificate data (galion.app leaf + Google Trust Services WE1 intermediate as served to any client) | captured 2026-10-20 23:19 CEST (+02:00) from galion.app:443 | real-format parser fixture | n/a (data, no secret: what every visitor receives) |
