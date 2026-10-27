<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes owner-notify channel (publish blocker B5)

Branch `swarm/hermes-telegram-notify` (wave-4 slot 266), base main-staging
7c95348b. Code: `crates/hermes-rs/src/notify.rs`, route in
`crates/hermes-rs/src/api.rs`, CLI in `crates/hermes-gateway/src/main.rs`.

Until now Hermes proposals (hoff-*) end with `notify = "no-channel-configured"`
and the owner only hears about them through Grok Bot. This adds a real
sender: Telegram Bot API `sendMessage`, the same channel galion-nerve and the
cortex dispatcher already expect (same env names, same status strings).

## Configuration (env only)

| Env | Meaning |
|---|---|
| `TELEGRAM_BOT_TOKEN` | Bot API token `<digits>:<token>`. Never printed, logged, returned or put on argv. |
| `TELEGRAM_CHAT_ID` | Owner chat: `-?digits` or `@channelname`. `TELEGRAM_HOME_CHANNEL` is accepted as a fallback (galion-nerve name). |
| `HERMES_RS_NOTIFY_PER_HOUR` | Max sends per rolling hour, default 6, hard cap 30. Failed attempts count too, so a broken API is never hammered. |
| `HERMES_RS_TELEGRAM_API_BASE` | Tests/mocks only. Must be loopback (`http://127.0.0.1:…`, `localhost`, `[::1]`) or exactly `https://api.telegram.org`; anything else = `bad-api-base`, nothing sent, so a bad setting can never hand the token to another host. |

Both token and chat missing (or either one) = **no channel**: every send is a
logged no-op. Nothing is read from files and nothing is written; no secret
goes into git. As of 2026-10-27 the VPS has no `TELEGRAM_*` values (Mate,
t274u), so the live default stays the no-op.

## Interfaces

* `POST /v1/notify` `{"text": "...", "kind": "proposal"}` — gateway key
  (`Authorization: Bearer $API_SERVER_KEY`) required, like every `/v1` route.
  Always `200 {"notify": "<outcome>"}` once the body is valid (plus `detail`
  on `telegram-failed`), so callers record the outcome instead of retrying.
  400 without `text`, 413 above 64 KiB, 401 without the key.
* `GET /v1/notify/status` — `{"channel", "api_base_default", "per_hour",
  "sent_last_hour"}`; never the token or the chat id.
* `hermes-gateway notify [--kind K] [--text T]` — one send straight from the
  env (no config, no port, no running gateway needed); text from stdin
  without `--text`. Prints the outcome JSON. Exit 0 = `telegram-sent` or
  `no-channel-configured`, 1 = any other outcome, 2 = usage.
* Library: `hermes_rs::notify::Notifier::from_env(&get, Box<dyn Transport>)`,
  `send(kind, text) -> Outcome`; `Transport` is a trait so tests use a mock.
* Gateway start logs one line: `notify: "telegram"` or
  `notify: "no-channel-configured"` (or the config error, e.g. `"bad-token"`).

## Outcomes (stable strings)

`no-channel-configured`, `telegram-sent`, `telegram-failed`, `bad-token`,
`bad-chat`, `bad-api-base`, `rate-limited`, `empty-text`. The first three match
`gaia-cortex` `dispatch.rs`.

## Safety

* Transport = system `curl` (no TLS crate, same as `CurlProvider`). The URL
  (which carries the token, as the Bot API requires), headers and JSON body
  go in a curl config on **stdin**; argv is only
  `-sS --connect-timeout 10 --max-time 20 -K -`. The real test scans every
  `/proc/*/cmdline` while curl is connected and finds no token.
* Every error string is scrubbed of the token (whole and the part after `:`)
  and passed through `hermes_rs::redact`.
* Log lines carry only `kind`, character count and outcome — never the text
  (it may describe incidents) and never the token.
* Text is run through `redact` (API keys, bearer tokens, `*_KEY=` pairs),
  control characters dropped, prefixed `[hermes] `, capped at 3800 chars.
* Token shape check (`digits:[A-Za-z0-9_-]{8,}`) and chat shape check before
  anything is sent.

## Not done here (owner decisions)

* **D-266-1** Put `TELEGRAM_BOT_TOKEN` / `TELEGRAM_CHAT_ID` into the hermes-rs
  unit env on the VPS (needs a bot + chat from Slavik/Mate; live config
  change, not done by this slot).
* **D-266-2** Who sends: the cortex dispatcher has its own Telegram sender
  (`dispatch.rs notify`). Options: keep both (two env copies of the token),
  or have the cortex call Hermes `POST /v1/notify` so the token lives in one
  unit only (recommended; small cortex change, separate slot).
* **D-266-3** Automatic hooks inside Hermes (e.g. enroll request pending,
  spend cap reached, coding tool refusal) — not wired yet; the route and
  library are ready for them.
* **D-266-4** Deploy of a hermes-gateway build with this route (live is
  aa77864 without `/v1/notify`).
