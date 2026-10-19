<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Speech adapter

- DESK-ID: openmind.B62
- Written: 2026-10-19T16:49:51+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: `crates/openmind-speech`

`SpeechProvider::uses_gpu` is always false. The crate is a CPU STT/TTS adapter that only parses fixture outputs. It does not spawn whisper.cpp, piper, or any model, and it does not download weights. An endpoint string is stored for a later registry row and is never dialed. There is no socket and no process. A later sandbox would own any child; this library does not.

Whisper.cpp fixtures are the `-oj` / `-ojf` JSON file (`transcription`, `offsets` already in milliseconds) and the whisper-server JSON body (`text`, optional `segments` in seconds). A `systeminfo` string that names a GPU does not change `uses_gpu`. `params.model` is stored and not opened.

Piper fixtures are a PCM WAV file (the `--output_file` bytes; samples are not copied or played) and the CLI log line `Real-time factor: R (infer=I sec, audio=A sec)`. `Loaded voice in N second(s)` is optional. Raw PCM, ONNX weights, and voice JSON configs are not opened.

`spawns_process`, `dials_network`, and `downloads_models` are always false. The usual whisper-server port `8080` is a constant only. This crate does not bind it.
