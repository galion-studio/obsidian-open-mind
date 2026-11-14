<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# openmind-desktop

Local control console for the Obsidian Open Mind desktop. The page is a command surface: a stack list, Open Mind as the presence, and one prompt. Printing the page opens no socket. `--listen` binds a loopback address only.

This page covers install, run, the loopback-only security model, and the seven Galion tones at 432 Hz. Packaging writes a tarball on this machine. It does not publish a release.

The crate description is in `crates/openmind-desktop/Cargo.toml` (the `description` field). Stack rows are `staged` or `planned`, never `live` (`crates/openmind-desktop/src/stack.rs`, `LAYERS`).

## Install

From a checkout of this repository, on Linux:

```sh
nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked
```

The binary is `target/release/openmind-desktop`. The crate depends on `galion-common`, and on `libc` for the Linux `O_NOFOLLOW` value (`crates/openmind-desktop/Cargo.toml`, `open_nofollow` in `crates/openmind-desktop/src/files.rs`).

A local package (tarball and checksums, kept on this machine):

```sh
scripts/desktop/package-local.sh
scripts/desktop/verify-local.sh target/openmind-desktop-dist
```

`package-local.sh` writes `target/openmind-desktop-dist/openmind-desktop-<version>-<host>.tar.gz` and `SHA256SUMS`. `target/` is gitignored, and the script does not write into `dist/`. The archive holds the release binary, `LICENSE`, this README, `console.html` (page mode, no socket), and `LOCAL-ARTIFACT.txt`.

The scripts refuse `--publish`, `--upload`, `--gh-release`, and `--cargo-publish`, and they refuse `OPENMIND_DESKTOP_PUBLISH=1`. They do not create a GitHub Release and they do not publish a crate. A release still needs Slavik's written go-ahead.

`--out DIR` chooses another local directory. `--target TRIPLE` passes that triple to cargo when the toolchain already has it. `--skip-build` packages a release binary that is already built.

## Run

Print the page and exit. This opens no socket (`crates/openmind-desktop/src/cli.rs`, `Mode::Page`, and the usage text):

```sh
./target/release/openmind-desktop > console.html
```

Serve on the default loopback address. `DEFAULT_BIND` is `127.0.0.1:18081` (`crates/openmind-desktop/src/lib.rs`):

```sh
./target/release/openmind-desktop --listen
```

Open `http://127.0.0.1:18081/` in a browser on this machine. The process prints the bound address on stderr (`crates/openmind-desktop/src/http.rs`, `listen`).

Another loopback address, including a free port:

```sh
./target/release/openmind-desktop --listen 127.0.0.1:0
./target/release/openmind-desktop --listen '[::1]:18081'
```

`--help` and `--version` print usage and the version line. They do not listen (`crates/openmind-desktop/src/cli.rs`, `Mode::serves`).

A non-loopback address is refused before a socket is created. Exit code 2 is a bad command or a refused address. Exit code 1 is a loopback bind the OS rejected (`crates/openmind-desktop/src/error.rs`, `exit_code`).

## Loopback-only security model

Only `--listen` may open a socket. With no flags the binary prints the page and exits (`crates/openmind-desktop/src/main.rs`).

`parse_loopback` accepts the address only when `ip().is_loopback()` is true (`crates/openmind-desktop/src/http.rs`). `127.0.0.1` and `::1` pass. `0.0.0.0`, a public address, and `[::]` are refused, and a string that is not `host:port` is a usage error. The tests for that refusal are in the same file (`refuses_non_loopback_before_bind`).

The listener answers `GET` and `HEAD` of `/` and `/index.html` with the control console. `/one` and `/research` are the Galion One and Galion Research entries. The Open Mind stack list stays on `/`. `GET /probes` is the read-only probe report. `GET` and `POST` under `/files` are the file gate. Any other path is 404. A method that does not match the path is 405 (`reply` in `crates/openmind-desktop/src/http.rs`). File uploads use a body cap of 8 MiB. Probe reads stay small (`listen` in the same file).

Response headers are `content-security-policy`, `referrer-policy: no-referrer`, `x-content-type-options: nosniff`, and `cache-control: no-store`. The policy is `default-src 'none'`, inline style and script only, `media-src data:`, `img-src data:`, `connect-src 'none'`, `base-uri 'none'`, `form-action 'none'`, `frame-ancestors 'none'` (`CSP` in `crates/openmind-desktop/src/page.rs`). The page repeats that policy in a meta tag and sets `Permissions-Policy` to `microphone=(self)` (`render` in the same file). `connect-src 'none'` means the page cannot open its own network connection.

The page keeps file names and commands in the document. The plus is `<input type="file" multiple>` with no `accept` attribute. The script copies `file.name` into a list with `textContent`. It does not read file bytes (`FileReader` is absent). Enter on the command field appends the text to "Kept on this page" or plays deny when the field is empty. The page source contains no `http://`, `https://`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `MediaRecorder`, `indexedDB`, `localStorage`, `createObjectURL`, or `sendBeacon` (`page_stays_on_the_machine` in `crates/openmind-desktop/src/page.rs`). No model is called (the presence copy in `render` in the same file). `POST` and `PUT`, including `/mic` and `/audio`, are 405 (`reply` in `crates/openmind-desktop/src/http.rs`).

The microphone is opt-in and loopback-only. The checkbox `#mic-opt` starts unchecked (`render`, and `microphone_is_opt_in_and_loopback_only`). The voice button calls `startMic` only after that box is ticked and `loopbackPage()` is true: a `file:` page, or a hostname of `localhost`, `127.0.0.1`, `::1`, or `[::1]` (`loopbackPage` and `startMic` in `crates/openmind-desktop/src/page.rs`). It then calls `getUserMedia({ audio: true, video: false })`. Samples sit in one reused `Float32Array` the size of `fftSize` (256) for a level number; the next tick overwrites that buffer (`pump`). Closing the button, or clearing the box, calls `stopMic`, which stops every track. The label says the audio is not saved and is not sent. The crate docs state the same (`crates/openmind-desktop/src/lib.rs`). The Voice stack row is `staged` with that role (`crates/openmind-desktop/src/stack.rs`).

Stack rows do not probe other programs. Nothing on the list is `live` (`crates/openmind-desktop/src/stack.rs`).

## Seven Galion tones at 432 Hz

Every pitch is an integer ratio of 432 Hz. `REFERENCE_HZ` is 432. `hz` is `REFERENCE_HZ * num / den`, and each ratio divides 432 exactly (`crates/openmind-desktop/src/sound.rs`, `REFERENCE_HZ`, `Tone::hz`, and `TONES`). The clips are 16-bit mono WAV at 16 kHz (`SAMPLE_RATE`), one sine with an 8 ms fade, peak amplitude 0.32. They are embedded in the page as `data:audio/wav;base64,...` (`crates/openmind-desktop/src/page.rs`, the tone loop in `render`). Nothing is fetched.

| id | ratio | Hz | ms | role | when the page plays it |
| --- | --- | --- | --- | --- | --- |
| wake | 1/1 | 432 | 160 | The console is ready | in the set and in the page; the script does not call it |
| ask | 5/4 | 540 | 120 | Voice armed, or a question | the microphone opens after the box is ticked on a loopback or file page |
| hear | 9/8 | 486 | 100 | A file name was kept on the page | the file input changes and at least one name is kept |
| notice | 4/3 | 576 | 140 | The stack wants a look | in the set and in the page; the script does not call it |
| done | 3/2 | 648 | 160 | A command was kept on the page | Enter on a non-empty command |
| deny | 3/4 | 324 | 180 | Refused. Nothing was sent | empty command, box not ticked, page not on loopback, no microphone API, or permission refused |
| close | 2/3 | 288 | 140 | Voice disarmed | the voice button closes the microphone |

Roles, ratios, and lengths are the `TONES` table in `crates/openmind-desktop/src/sound.rs`. The script calls are `play("hear")`, `play("ask")`, `play("close")`, `play("deny")`, and `play("done")` in `crates/openmind-desktop/src/page.rs`. Mute (`aria-pressed` on `#mute`) and local hours 22:00–06:59 skip playback (`night` and `play` in that script). `wake` is checked against 432 Hz, not concert 440 (`wake_is_432_not_440` in `sound.rs`).

## CI

`.github/workflows/openmind-desktop.yml` runs on Linux (`ubuntu-latest`). The `linux` job builds the crate, runs clippy with `-D warnings` on all targets, runs the tests, then `package-local.sh` and `verify-local.sh` with the output under the runner temp directory. The `linux-aarch64` job cross-builds `aarch64-unknown-linux-musl` (linker `rust-lld`, `.cargo/config.toml`) and runs the same tests under qemu-user (`qemu-aarch64-static` or `qemu-aarch64`, the same package split as the self-eval `ucpu-qemu` job). That is the run that checks `O_NOFOLLOW` (`nofollow_flag_matches_this_target` in `crates/openmind-desktop/src/files.rs`): the flag is `0x8000` on aarch64 and `0x20000` on x86. Workflow permissions are `contents: read`. The jobs do not upload the tarball and do not create a release. A publish flag must exit 2.
