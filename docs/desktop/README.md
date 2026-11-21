<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# openmind-desktop

Local control console for the Obsidian Open Mind desktop. The page is a command surface: a stack list, Open Mind as the presence, and one prompt. Printing the page opens no socket. `--listen` binds a loopback address only.

This page covers install, run, the loopback-only security model, and the seven Galion tones at 432 Hz. Packaging writes archives on this machine. CI uploads those archives as workflow artifacts. It does not publish a release.

The crate description is in `crates/openmind-desktop/Cargo.toml` (the `description` field). Stack rows are `staged` or `planned`, never `live` (`crates/openmind-desktop/src/stack.rs`, `LAYERS`).

## Install

From a checkout of this repository, on Linux:

```sh
nice -n 19 cargo build -j 2 -p openmind-desktop --release --locked
```

The binary is `target/release/openmind-desktop`. The crate depends on `galion-common`, and on `libc` for `O_NOFOLLOW` on Linux and macOS (`crates/openmind-desktop/Cargo.toml`, `open_nofollow` in `crates/openmind-desktop/src/files.rs`). Windows passes `FILE_FLAG_OPEN_REPARSE_POINT` in that function.

A local package (tarball and checksums, kept on this machine):

```sh
scripts/desktop/package-local.sh
scripts/desktop/verify-local.sh target/openmind-desktop-dist
```

`package-local.sh` writes `target/openmind-desktop-dist/openmind-desktop-<version>-<host>.tar.gz` and `SHA256SUMS`. `target/` is gitignored, and the script does not write into `dist/`. The archive holds the release binary, `LICENSE`, this README, `console.html` (page mode, no socket), and `LOCAL-ARTIFACT.txt`.

The scripts refuse `--publish`, `--upload`, `--gh-release`, and `--cargo-publish`, and they refuse `OPENMIND_DESKTOP_PUBLISH=1`. They do not create a GitHub Release and they do not publish a crate. A release still needs Slavik's written go-ahead.

A downloadable workflow artifact is a separate directory, `target/openmind-desktop-ci`:

```sh
scripts/desktop/package-artifact.sh
scripts/desktop/check-package.sh target/openmind-desktop-ci
```

`package-artifact.sh` writes `LICENSE` (the root MIT text plus the four Galion Labs lines in `scripts/desktop/brand-lines.txt`, the same lines as `BRAND_LINES` in `crates/openmind-desktop/src/page.rs`), a short README (`scripts/desktop/artifact-readme.md`), `SHA256SUMS`, and one archive. On a Linux triple the archive is a `.tar.gz` and the directory also contains a `.deb` of the same binary; that `.deb` is inside the tarball as well. A `*-pc-windows-msvc` triple is a `.zip`. An `*-apple-darwin` triple is a `.tar.gz`. `--target`, `--skip-build`, `--runner`, and `--out` are the local knobs. The script refuses `--publish`, `--upload`, `--gh-release`, `--cargo-publish`, `--sign`, and `--signed`, and it refuses `OPENMIND_DESKTOP_PUBLISH=1` and `OPENMIND_DESKTOP_SIGN=1`. It does not upload the directory and it does not sign it. CI uploads that directory with `actions/upload-artifact` after `check-package.sh` passes.

`--out DIR` chooses another local directory. `--target TRIPLE` passes that triple to cargo when the toolchain already has it. `--skip-build` packages a release binary that is already built.

## Ubuntu 24.04 WSL2

Inside the Ubuntu 24.04 WSL2 distro named Galion-Ubuntu, from a checkout of this repository:

```sh
sudo scripts/install-wsl-ubuntu.sh
```

`scripts/install-wsl-ubuntu.sh` installs the build packages and, when `rustc` is older than 1.88, an upstream rustup stable toolchain for the root user. `crates/galion-common/src/hex.rs` calls `as_chunks`, which is why 1.88 is the floor. The Ubuntu 24.04 `rustc` package is older. The script does not install the Ubuntu `rustup` package, because that package conflicts with `rustc`. It then runs `scripts/desktop/package-artifact.sh`, checks the directory with `scripts/desktop/check-package.sh`, and installs the Linux `.deb` with `dpkg -i`. That package places `openmind-desktop` in `/usr/bin`. A second run writes the same paths and exits 0. The script refuses the same publish and sign flags as `package-artifact.sh`.

WSLg reads `/usr/share/applications/openmind-desktop.desktop` and lists it in the Windows Start menu for this distro. `Exec` is `/usr/bin/openmind-desktop-wslg`, which is `scripts/desktop/wslg-launch.sh`. The launcher starts `openmind-desktop --listen` and opens the URL from `listen_banner` (`crates/openmind-desktop/src/http.rs`) in a browser on this distro. That browser window is the WSLg window. The listener log is `$HOME/.local/state/openmind-desktop/wslg.log`.

`openmind-desktop` is not a Chromium or Electron process. `--no-sandbox` is a usage error for it (exit 2, `DesktopError::Usage` in `crates/openmind-desktop/src/error.rs`). The launcher never passes that switch to the binary. `chromium_needs_no_sandbox` in `scripts/desktop/wslg-launch.sh` adds it only for a Chromium-family browser: `OPENMIND_WSL_NO_SANDBOX=1` forces it, `OPENMIND_WSL_NO_SANDBOX=0` forbids it, and otherwise the launcher adds it when that browser has no setuid `chrome-sandbox` and either `/proc/sys/kernel/apparmor_restrict_unprivileged_userns` is `1` or `unshare --user --map-root-user true` fails. Firefox and Epiphany do not get the switch.

On WSL, if no usable browser is on `PATH`, the script installs the `epiphany-browser` package. A snap shim is not usable. `--no-browser` skips the package. `--with-browser` installs it even when a browser is already present. `--smoke` skips it.

The browser keeps the WSLg GPU path. `OPENMIND_WSL_SOFTWARE_GL=1` sets `LIBGL_ALWAYS_SOFTWARE=1` for the browser process only. `OPENMIND_WSL_WEBKIT_SOFTWARE=1` sets `WEBKIT_DISABLE_COMPOSITING_MODE=1` for Epiphany. `OPENMIND_WSL_X11=1` sets `GDK_BACKEND=x11` and `MOZ_ENABLE_WAYLAND=0`, and adds `--ozone-platform=x11` for Chromium. The listener is started before those variables are exported.

## Run

Print the page and exit. This opens no socket (`crates/openmind-desktop/src/cli.rs`, `Mode::Page`, and the usage text):

```sh
./target/release/openmind-desktop > console.html
```

Opening that file writes `This is a printed page. Look and uploads need the loopback listener.` into `#printed` (`render` in `crates/openmind-desktop/src/page.rs`). The line stays hidden on the listener (`#printed:empty` in the page style). The microphone still follows the opt-in rule on a `file:` page.

Serve on the default loopback address. `DEFAULT_PORT` is 18181 and `DEFAULT_BIND` is `127.0.0.1:18181` (`crates/openmind-desktop/src/lib.rs`). Port 18081 is the staging page beside the Rust console on 18080, so the desktop does not bind it. This listener is not the console probe. The console probe stays on 18080 (`crates/openmind-desktop/src/probe.rs`, `Service::default_port`).

```sh
./target/release/openmind-desktop --listen
```

Open `http://127.0.0.1:18181/` in a browser on this machine. The process prints the bound address on stderr (`crates/openmind-desktop/src/http.rs`, `listen`). The page writes `Listening on ` plus `location.host` into `#here` when the protocol is not `file:` and `loopbackPage()` is true (`render` in `crates/openmind-desktop/src/page.rs`). The line stays hidden on a printed page. The same check writes `here` on the Open Mind row. Look clears every `.seen` span and then writes `here` again, because Open Mind is not a probe. The row stays `staged`.

The port is configurable. `--port` and `OPENMIND_DESKTOP_PORT` both bind `127.0.0.1` on that port. An address given to `--listen` wins, and `--port` cannot be combined with that address. A blank environment value is unset. A value that is not a port from 1 to 65535 is a usage error (exit 2). The binary reads the variable (`crates/openmind-desktop/src/main.rs`, `listen_port_from_env`). `parse` does not (`crates/openmind-desktop/src/cli.rs`, `resolve_listen_addr`).

```sh
./target/release/openmind-desktop --listen --port 19000
OPENMIND_DESKTOP_PORT=19000 ./target/release/openmind-desktop --listen
```

Another loopback address, including a free port or IPv6. The port flag and the environment variable do not change the host; use `--listen` for `::1`.

```sh
./target/release/openmind-desktop --listen 127.0.0.1:0
./target/release/openmind-desktop --listen '[::1]:18181'
```

`--help` and `--version` print usage and the version line. They do not listen (`crates/openmind-desktop/src/cli.rs`, `Mode::serves`).

A non-loopback address is refused before a socket is created. Exit code 2 is a bad command or a refused address. Exit code 1 is a loopback bind the OS rejected (`crates/openmind-desktop/src/error.rs`, `exit_code`).

## Loopback-only security model

Only `--listen` may open a socket. With no flags the binary prints the page and exits (`crates/openmind-desktop/src/main.rs`).

`parse_loopback` accepts the address only when `ip().is_loopback()` is true (`crates/openmind-desktop/src/http.rs`). `127.0.0.1` and `::1` pass. `0.0.0.0`, a public address, and `[::]` are refused, and a string that is not `host:port` is a usage error. The tests for that refusal are in the same file (`refuses_non_loopback_before_bind`).

The listener answers `GET` and `HEAD` of `/` and `/index.html` with the control console. `/one` and `/research` are the Galion One and Galion Research entries (`entry` in `crates/openmind-desktop/src/page.rs`). Each entry is its own page: a title, one sentence, a link back to `/`, the four brand lines, and the MIT text. The Open Mind stack list stays on `/`. The entries do not include it (`one_and_research_are_entries_and_the_stack_stays_on_the_console`). `GET /probes` is the read-only probe report. `GET /files` lists chosen paths. `GET /files/sent` lists what was stored. `POST /files/read` reads one chosen path. `POST /files` uploads picker bytes, or a chosen path when the body is `{"path":"..."}`. Any other path is 404. A method that does not match the path is 405, including `PUT` and a `POST` to `/`, `/mic`, or `/audio` (`reply` in `crates/openmind-desktop/src/http.rs`). A peer that is not loopback is 403 before a file is touched.

File uploads are capped at 8 MiB. `MAX_FILE_BYTES` is `8 * 1024 * 1024` (`crates/openmind-desktop/src/files.rs`). The listener uses that as `max_body_bytes` (`listen` in `crates/openmind-desktop/src/http.rs`). The page script uses the same number as `FILE_MAX` (`render` in `crates/openmind-desktop/src/page.rs`). An empty file or a larger file is not sent. The plus is `<input id="files" type="file" multiple>` with no `accept` attribute. On change, each name is shown with `textContent` on an `li` of class `chip` inside `<ul class="chips" id="picked" aria-label="Attached files">`, then the script `fetch("/files")` with `method: "POST"`, `credentials: "omit"`, header `X-File-Name`, and the file as the body. The other `fetch(` is `GET /probes`, and only after Look. There is no `FileReader`. The server still checks the extension allow-list and the magic bytes (`ALLOWED` and `classify` in `crates/openmind-desktop/src/files.rs`): `txt`, `md`, `json`, `csv`, `log`, `pdf`, `jpg`, `jpeg`, `png`, `webp`. Probe reads stay small (`MAX_BODY` in `crates/openmind-desktop/src/probe.rs`).

Plain `--listen` dials the stack intake and does not open a path. `listen` builds `Files` with no grants, no inbox, and `DEFAULT_STACK` (`listen` in `crates/openmind-desktop/src/http.rs`). A path read is refused. Picker bytes are still accepted, then forwarded. See [Stack intake](#stack-intake).

Response headers are `content-security-policy`, `referrer-policy: no-referrer`, `x-content-type-options: nosniff`, and `cache-control: no-store`. The policy is `default-src 'none'`, inline style and script only, `media-src data:`, `img-src data:`, `connect-src 'self'`, `base-uri 'none'`, `form-action 'none'`, `frame-ancestors 'none'` (`CSP` in `crates/openmind-desktop/src/page.rs`). `connect-src 'self'` is what allows the one same-origin upload. Any other origin is refused. The console page repeats that policy in a meta tag and sets `Permissions-Policy` to `microphone=(self)` (`render` in the same file). `/one` and `/research` get the same CSP header from the listener (`entry_response` in `crates/openmind-desktop/src/http.rs`). The page source contains no `http://`, `https://`, `XMLHttpRequest`, `WebSocket`, `MediaRecorder`, `indexedDB`, `localStorage`, `createObjectURL`, or `sendBeacon` (`page_stays_on_the_machine` in `crates/openmind-desktop/src/page.rs`). No model is called (the presence copy in `render` in the same file). Enter on the command field appends the text to "Kept on this page" or plays deny when the field is empty. The command is not uploaded.

The microphone is opt-in and loopback-only, and the meter is the level line. The checkbox `#mic-opt` starts unchecked (`render`, and `microphone_is_opt_in_and_loopback_only`). The voice button and the command `voice` call `toggleVoice`. That calls `startMic` only after that box is ticked and `loopbackPage()` is true: a `file:` page, or a hostname of `localhost`, `127.0.0.1`, `::1`, or `[::1]` (`loopbackPage` and `startMic` in `crates/openmind-desktop/src/page.rs`). It then calls `getUserMedia({ audio: true, video: false })`. Samples sit in one reused `Float32Array` the size of `fftSize` (256). Each tick writes the RMS into `#mic-level` as `Level N` (`pump`). The next tick overwrites that buffer. Closing the button, typing `voice` again, or clearing the box, calls `stopMic`, which stops every track and clears the level line. The label says the audio is not saved and is not sent. The crate docs state the same (`crates/openmind-desktop/src/lib.rs`). The Voice stack row is `staged` with that role (`crates/openmind-desktop/src/stack.rs`). The command does not tick the box. When the microphone opens, that row's `.seen` span says `open`. `stopMic` clears `open` only, so a probe word left by Look stays until the next Look, unless the tones are muted. In that case it writes `muted`.

Typing `look` and pressing Enter keeps that word on the page and runs the same Look as the button. Typing `files` keeps that word and opens the same picker as the plus. Either click writes `open` on the Files row. Hear still plays when a file is chosen. Typing `voice` keeps that word and runs the same toggle as the voice button. Typing `mute` keeps that word and runs the same toggle as the Mute button. It plays no tone. Turning the tones off writes `muted` on the Voice row when the microphone is closed. Turning them on clears `muted` only. Opening the microphone still writes `open`. Closing it writes `muted` again when the tones are still muted. Look clears that word. The row stays `staged`. Any other non-empty command is kept and writes `kept` on the Command row. Look clears that word. The row stays `staged`. The presence names the four words in `#words`: `look` checks the stack, `files` opens the picker, `voice` toggles the microphone, and `mute` toggles the tones (`render` in `crates/openmind-desktop/src/page.rs`). Stack rows do not probe other programs. Nothing on the list is `live` (`crates/openmind-desktop/src/stack.rs`). Look writes `up`, `down`, `unknown`, or `off` into the empty `.seen` span on the row whose id matches the probe service (`nerve`, `hermes`, `voice`, `console`, `desk`, `memory`, `galion-one`). Desk is `GET /health` on port 8646 (`Service::Desk` in `crates/openmind-desktop/src/probe.rs`, the same port as `galion-desk` in `crates/galion-desk/src/main.rs`). The Desk row stays `staged`. Memory is `GET /health` on port 8420 (`Service::Memory` in the same file, the default in `crates/openmind-memory/src/config.rs`). The Memory row stays `planned`. Galion One is `GET /health` on port 8091 (`Service::GalionOne` in the same file, `DEFAULT_BIND` in `crates/galion-one/src/probe.rs`). The Galion One row stays `planned`. The `.state` span stays `staged` or `planned`. A probe with no row, such as cortex, is only a chip under the button. The ten rows are `LAYERS`, in this order:

| id | name | state | role |
| --- | --- | --- | --- |
| open-mind | Open Mind | staged | Main presence on this desktop |
| command | Command | staged | Centre field. Text stays on this page |
| files | Files | staged | Plus picks files. Read and upload stay on loopback |
| voice | Voice | staged | Opt-in microphone on loopback. Audio is not saved |
| galion-one | Galion One | planned | Voice surface. Not started by this page |
| hermes | Hermes | planned | Engine. Not started by this page |
| memory | Memory | planned | Long-term store. Not started by this page |
| nerve | Nerve | planned | Health monitor. Not started by this page |
| desk | Desk | staged | Read-only pod endpoint, separate program |
| console | Console | staged | Operator rooms, separate program |

`/research` is not a row. It is an entry page linked from the header. The Galion One row stays `planned`, and its name is a same-origin link to `/one` (`render` in `crates/openmind-desktop/src/page.rs`). A file the console accepts writes `sent` in that row's `.seen` span. The next Look clears `sent` and writes the Galion One probe word. The `.state` span stays `planned`. The plus and the command `files` write `open` in the Files row's `.seen` span, because both click the same picker. That row stays `staged`. Look clears `open` the same way. Opening the microphone writes `open` in the Voice row's `.seen` span. That row stays `staged`. Closing the microphone clears `open` only, then writes `muted` when the tones are still muted. Look clears it with the other `.seen` spans and then writes the voice probe word. Muting writes `muted` in that same span when the microphone is closed, and unmuting clears `muted` only. The header still links both `/one` and `/research`.

## Stack intake

`DEFAULT_STACK` is `127.0.0.1:8091` (`crates/openmind-desktop/src/files.rs`). That is Galion One's loopback `POST /files`, a dial, not a bind. The desktop listener stays on 18181.

`forward` connects only when the address is loopback, then `POST /files` with `Content-Type: application/octet-stream` and `X-File-Name` (`forward` in the same file). A connect failure is unreachable. A body containing `"error":"pair"` is a pairing refusal. The pairing token is not sent and is not a flag. Any other non-200, or a body containing `"error"`, is a stack status failure.

`--stack ADDR` chooses another loopback `host:port`. `--stack off` does not open a client. `--inbox DIR` stores an accepted file in that directory. `--grant PATH` is a file or directory the operator chose, and it is repeatable. Path reads need a grant. `--grant`, `--inbox`, `--port`, and `--stack` are refused without `--listen` (`USAGE` in `crates/openmind-desktop/src/cli.rs`).

With neither an inbox nor a stack address, an upload has no destination (`accept` in `crates/openmind-desktop/src/files.rs`).

## Galion Research

The web interface is https://github.com/galion-studio/hermes-research for research.galion.app. This desktop does not draw that prompt. The console room no longer draws it either. The earlier markup is `docs/archive/research-web/console-prompt.html`.

The desktop `/research` page is the entry above. It does not post a file.

`POST /hermes/attach` and `POST /research/attach` still keep a file on this machine (`crates/console-ui/src/attach.rs`) without dialing the research engine. The handler does not open a path from the request and does not put the bytes in the redirect. The cap is the same 8 MiB (`MAX_BYTES` in that file). Asking, runs, the microphone, and speak stay off that room (`crates/console-ui/src/pages/hermes.rs`).

## Seven Galion tones at 432 Hz

Every pitch is an integer ratio of 432 Hz. `REFERENCE_HZ` is 432. `hz` is `REFERENCE_HZ * num / den`, and each ratio divides 432 exactly (`crates/openmind-desktop/src/sound.rs`, `REFERENCE_HZ`, `Tone::hz`, and `TONES`). The clips are 16-bit mono WAV at 16 kHz (`SAMPLE_RATE`), one sine with an 8 ms fade, peak amplitude 0.32. They are embedded in the page as `data:audio/wav;base64,...` (`crates/openmind-desktop/src/page.rs`, the tone loop in `render`). Nothing is fetched.

| id | ratio | Hz | ms | role | when the page plays it |
| --- | --- | --- | --- | --- | --- |
| wake | 1/1 | 432 | 160 | The console is ready | once, when the page is the loopback listener and not a printed `file:` page |
| ask | 5/4 | 540 | 120 | Voice armed, or a question | the microphone opens after the box is ticked on a loopback or file page |
| hear | 9/8 | 486 | 100 | A file was picked for upload | the file input changes and at least one file is selected |
| notice | 4/3 | 576 | 140 | The stack wants a look | Look finds a probe that is not up, or no probe was enabled |
| done | 3/2 | 648 | 160 | A command was kept on the page | Enter on a non-empty command other than the words `look`, `files`, `voice`, and `mute` |
| deny | 3/4 | 324 | 180 | Refused. Nothing was sent | empty command, empty or oversize file, upload refused, box not ticked, page not on loopback, no microphone API, or permission refused |
| close | 2/3 | 288 | 140 | Voice disarmed | the voice button or the word `voice` closes the microphone |

Roles, ratios, and lengths are the `TONES` table in `crates/openmind-desktop/src/sound.rs`. The script calls are `play("wake")`, `play("hear")`, `play("ask")`, `play("close")`, `play("deny")`, `play("done")`, and `play("notice")` in `crates/openmind-desktop/src/page.rs`. `wake` runs only when `location.protocol` is not `file:` and `loopbackPage()` is true. Mute (`aria-pressed` on `#mute`) and local hours 22:00–06:59 skip playback (`night` and `play` in that script). Mute writes `muted` on the Voice row when the microphone is closed (`markMuted` in that script). During those hours the page writes `Tones are quiet until 07:00.` into `#quiet`. The line stays hidden while it is empty (`#quiet:empty, #printed:empty, #here:empty` in the page style). `wake` is checked against 432 Hz, not concert 440 (`wake_is_432_not_440` in `sound.rs`).

## CI

`.github/workflows/openmind-desktop.yml` runs five jobs. Workflow permissions are `contents: read` and `actions: write`. `actions: write` is the permission `actions/upload-artifact` uses. The workflow does not set `contents: write`, so it cannot create a GitHub Release. It does not sign packages and it does not publish a crate.

The `linux` job (`ubuntu-latest`) builds the crate, runs clippy with `-D warnings` on all targets, runs the tests, then `package-local.sh` and `verify-local.sh` with the output under the runner temp directory. It checks that `rustc -vV` reports `x86_64-unknown-linux-gnu`, then `package-artifact.sh --skip-build` and `check-package.sh` write and check `target/openmind-desktop-ci`. That directory is uploaded as `openmind-desktop-linux-x86_64`. A publish or sign flag must exit 2. A README that no longer matches `SHA256SUMS` must make `check-package.sh` exit 1. A LICENSE that is one line short, after `SHA256SUMS` is rewritten for that file, must also exit 1.

The `wsl-ubuntu` job runs in an `ubuntu:24.04` container. It runs `scripts/install-wsl-ubuntu.sh --smoke`, then runs that command again. `--smoke` requires `openmind-desktop --version` to print a version line, the same command under `xvfb-run -a` to exit 0, `desktop-file-validate` to accept the menu file, `openmind-desktop --no-sandbox` to exit 2, and the launcher to parse the banner from a short `--listen`. The container is not a WSLg session. The job does not upload an artifact.

The `linux-aarch64` job cross-builds `aarch64-unknown-linux-musl` (linker `rust-lld`, `.cargo/config.toml`) and runs the same tests under qemu-user (`qemu-aarch64-static` or `qemu-aarch64`, the same package split as the self-eval `ucpu-qemu` job). That is the run that checks `libc::O_NOFOLLOW` (`O_NOFOLLOW` and `nofollow_flag_matches_this_target` in `crates/openmind-desktop/src/files.rs`): `libc` supplies the flag, which is `0x8000` on aarch64 and `0x20000` on x86. On ARM, `0x20000` is `O_LARGEFILE`, so a hard-coded x86 value would not stop `open` from following a symlink. After the tests it release-builds the same target, packages the tarball and `arm64` `.deb` with `--runner` set to that qemu binary, checks the package, and uploads `openmind-desktop-linux-aarch64`.

Windows (`windows-latest`, target `x86_64-pc-windows-msvc`) and macOS (`macos-latest`, target `aarch64-apple-darwin`) build the crate, run clippy with `-D warnings` on all targets, and run the tests. After the tests, Windows writes a zip and uploads `openmind-desktop-windows-x86_64`. macOS writes a tarball and uploads `openmind-desktop-macos-aarch64`. macOS file opens use `libc::O_NOFOLLOW`, which is `0x0100` (`O_NOFOLLOW` and `nofollow_flag_matches_this_target` in `crates/openmind-desktop/src/files.rs`). Windows passes `FILE_FLAG_OPEN_REPARSE_POINT` (`0x00200000`) and refuses a handle that is a reparse point before any byte is read (`FILE_FLAG_OPEN_REPARSE_POINT`, `open_reparse_point_flag_matches_this_target`, and `open_reparse_point_does_not_read_a_symlink` in the same file). Any other target checks `symlink_metadata` and then opens the file.

Artifact retention is 14 days (`retention-days` on each `actions/upload-artifact` step in `.github/workflows/openmind-desktop.yml`). The upload action is pinned to `actions/upload-artifact` `ea165f8d65b6e75b540449e92b4886f43607fa02`.

The four uploaded directories are `openmind-desktop-linux-x86_64`, `openmind-desktop-linux-aarch64`, `openmind-desktop-windows-x86_64`, and `openmind-desktop-macos-aarch64`. Each one is a workflow artifact on the run, not a GitHub Release. Unpack it and run `./openmind-desktop` (Unix or macOS) or `.\openmind-desktop.exe` (Windows). On Debian or Ubuntu, `dpkg -i` installs the Linux `.deb`. The short instructions inside the artifact are `scripts/desktop/artifact-readme.md`.
