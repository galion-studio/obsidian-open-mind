<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hermes-telegram-notify (slot 266)

No new crates and no copied code. `Cargo.lock` is unchanged.

| What | Licence | Source | Why |
|---|---|---|---|
| system `curl` binary (runtime, already used by `CurlProvider`) | curl (MIT-style) | https://curl.se/docs/copyright.html | HTTPS without a TLS crate or C build dep; config on stdin keeps the token off argv. Same pattern as the existing provider transport and galion-nerve alerts. |
| Telegram Bot API `sendMessage` (protocol, not code) | n/a | https://core.telegram.org/bots/api#sendmessage | The channel the nerve and cortex already expect (`TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`). |

Reuse before writing: the curl-on-stdin transport, `curl_quote` and `redact`
are the existing hermes-rs ones; the env names, chat fallback
(`TELEGRAM_HOME_CHANNEL`) and outcome strings follow galion-nerve
`alerts.rs` and gaia-cortex `dispatch.rs`. A Telegram client crate (e.g.
teloxide, MIT) was not taken: it pulls an async runtime and an HTTP/TLS
stack for one POST.
