<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party components: galion-stt

Written 2026-10-29 09:25 CET (+01:00), Europe/Zagreb.

## Rust crates (direct)

| Crate | Version | Licence | Why |
|---|---|---|---|
| axum | 0.8.9 | MIT | HTTP + WebSocket server (`http1`, `tokio`, `ws`, `json` only) |
| tokio | 1.54.1 | MIT | async runtime |
| tokio-tungstenite | 0.29 | MIT | WS client for `probe` and the real tests (same version axum uses, no duplicate) |
| futures-util | 0.3 | MIT OR Apache-2.0 | stream split for the client (already in the tree via axum) |
| libloading | 0.8.9 | ISC | dlopen of libmoonshine |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | pinned file checks |
| serde, serde_json | workspace | MIT OR Apache-2.0 | protocol JSON |

The whole normal dependency tree is 87 crates. Every licence expression allows MIT, Apache-2.0, BSD, ISC, Zlib or
Unicode-3.0 (one is `Unlicense OR MIT`, taken as MIT). This was checked with `cargo tree --format '{l}'` on
2026-10-29; `cargo deny` is not installed on the box.

## Native runtime (not linked at build time, loaded from `/opt/galion/stt-moonshine/lib`)

| Component | Version | Licence | Source |
|---|---|---|---|
| Moonshine (libmoonshine.so, C API) | 0.1.5, repo commit 234f60f (2026-08-24) | MIT | https://github.com/moonshine-ai/moonshine (release `moonshine-voice-linux-x86_64.tar.gz`) |
| Moonshine Tiny Streaming English weights | quantized_26_08_21 (8-bit) | MIT | https://download.moonshine.ai/model/tiny-streaming-en/quantized_26_08_21/ |
| ONNX Runtime | 1.23.2 | MIT | https://github.com/microsoft/onnxruntime/releases/tag/v1.23.2 |

The sha256 of every file is in `docs/galion-stt.md` and `crates/galion-stt/src/models.rs`.

## Test audio (box only, not committed)

- `jfk.wav`: JFK inaugural address, 1961. US public domain (via the whisper.cpp samples). sha256
  `59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e`.
- `two_cities_16k.wav`: LibriVox reading of *A Tale of Two Cities*. Public domain (moonshine test-assets at
  234f60f). sha256 `14a5a09479cab470db7de4fd16c0e18a8a88c03cc6d92a09a658e857ee1f73a5`.
