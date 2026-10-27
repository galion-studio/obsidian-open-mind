<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: hermes-telegram-notify (wave-4 slot 266)

Times Europe/Zagreb, CET +01:00. Box only for every send; the VPS was probed
read-only. No real message was sent to Telegram; no secret was used, printed
or stored (the test token is random per run and lives in test memory only).

## Box: real binary + mock Bot API (GALION_REAL=1)

`GALION_REAL=1 cargo test --release -p hermes-gateway --test real_telegram_notify -- --nocapture`
(ports 21360-21364, loopback). Release `hermes-gateway --mock`
(`HERMES_RS_PROVIDER_LOOPBACK_ONLY=1`), mock Telegram server in the test.
Run 2026-10-27T18:29:06+01:00, then 3 more passes 18:31-18:32 (with the
argv-scanner control added), 4/4 ok, ~0.9 s each.

```
no-channel: status={"channel":"no-channel-configured","api_base_default":null,"per_hour":6,"sent_last_hour":0} post={"notify":"no-channel-configured"} unauth=401 missing-text=400 mock_hits=0
channel: sent in 8 ms, mock got POST /bot<token>/sendMessage chat_id=-1001234567, argv_leak=false
429: {"notify":"telegram-failed","detail":"http 429 Too Many Requests: retry after 7 retry_after=7s"}; 3rd={"notify":"telegram-sent"}; 4th={"notify":"rate-limited"}; status={"channel":"telegram","api_base_default":false,"per_hour":3,"sent_last_hour":3}
bad-api-base: status={"channel":"bad-api-base","api_base_default":null,"per_hour":3,"sent_last_hour":0} post={"notify":"bad-api-base"}
cli: --text exit 0 {"notify":"telegram-sent"}; stdin exit 0 {"notify":"telegram-sent"}; no-env exit 0 {"notify":"no-channel-configured"}; closed port exit 1 {"notify":"telegram-failed","detail":"curl exit Some(7): curl: (7) Failed to connect to 127.0.0.1 port 21364 after 0 ms: Could not connect to server"}; bad arg exit 2
token_in_logs_or_stdout=false mock_hits_total=5
```

Checked in the test: the gateway log has `notify: "no-channel-configured"` /
`notify: "telegram"` at start and `kind=proposal chars=19
outcome=no-channel-configured (no-op…` per send, never the text; the mock
receives the exact Bot API body (`chat_id`, `[hermes] `-prefixed text with
quotes and newline intact, `disable_web_page_preview`); while curl is
connected the mock scans every `/proc/*/cmdline` for the token (control: a
`sh` with a random marker on argv IS found) and finds none; the token (and
its secret half) is in no log, stdout or reply; the 4th send under
`HERMES_RS_NOTIFY_PER_HOUR=3` never leaves the box; a non-loopback API base
sends nothing; all gateways stopped and ports closed at the end.

## Box: real Bot API endpoint, invalid token (no message possible)

2026-10-27 18:30:35 +01:00, release binary, `TELEGRAM_BOT_TOKEN=1:invalidtokenfortest`
(not a real token), real `https://api.telegram.org`:

```
hermes-rs notify kind=probe chars=43 outcome=telegram-failed detail=http 401 Unauthorized
{"notify":"telegram-failed","detail":"http 401 Unauthorized"}
exit=1
```

Proves the curl-on-stdin transport, TLS, `write-out` status parsing and the
Bot API error parsing against the real server. Nothing can be delivered with
that token.

## Static builds

x86_64 musl (`--no-default-features`) 1,217,536 B: `env -i hermes-gateway
notify --text hi` -> `{"notify":"no-channel-configured"}`, exit 0, log line
with the no-op hint. aarch64 musl (`--no-default-features`, rust-lld)
1,020,592 B built; qemu-aarch64 is not installed on the box, so not run.
aarch64-linux-android and armv7-unknown-linux-musleabihf `cargo check` ok.
x86_64 musl with default features (bundled SQLite C) needs
`x86_64-linux-musl-gcc`, which the box lacks — same as base 7c95348b.

## VPS (read-only), 2026-10-27 18:30:51 +01:00

| Probe | Result |
|---|---|
| live hermes-rs `GET 127.0.0.1:8642/health` | 200 `{"status":"ok",…,"implementation":"hermes-rs","implementation_version":"0.1.0"}` |
| live hermes-rs `POST /v1/notify` without key | 401 (auth wall before routing; nothing changed; live build has no `/v1/notify` yet) |
| nerve `GET 127.0.0.1:8670/state` | score 78, grade C, cycle 4, mode live |
| `systemctl is-active hermes-rs galion-nerve gaia-cortex` | active active active |

Disclosure: the `/state` body was saved to `/tmp/.s266` on the VPS for
grepping and removed in the same command. No service, unit, config or file
under /opt/galion was touched; no `TELEGRAM_*` value was looked for or read
(Mate t274u: none exist on the VPS).

## Gates (box, 2026-10-27 18:31-18:46 +01:00)

* `cargo fmt --check` ok; `cargo clippy -p hermes-rs -p hermes-gateway --all-targets -- -D warnings` 0 warnings.
* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --no-fail-fast`: exit 0, 375 suites, 1982 passed, 0 failed, 18 ignored (includes 9 new `notify::tests`).
* `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 737 scanned, 0 missing; `repo-check` 0 errors, 79 warnings (none in the new files) -> PASS.
* No sandbox test run (sandbox_vps_only).
