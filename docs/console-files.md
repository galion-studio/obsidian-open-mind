<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# console-files: the `/files` explorer (2026-11-01 CET)

Branch `swarm/console-files-20261101`. A Rust file explorer, previewer and
editor for the galion.app console, plus the same operations as Hermes tools.
Nothing here is deployed; turning it on needs Slavik's yes (see the end).

## What it does

`galion-console-static ... --local-api --files DIR` adds a page at `/files`
(phone first, two panes from 860 px) behind the existing pairing gate:

| File | Preview |
| --- | --- |
| folders | list (dirs first, dotfiles behind a toggle), filter, breadcrumbs |
| images (png, jpg, gif, webp, avif, svg, bmp, ico, …) | `<img>`; SVG never runs script (raw CSP `sandbox`) |
| video (mp4, webm, mov, mkv, …) | `<video playsinline controls>` with Range seeking; posters + duration/codec when `--files-ffmpeg` is set |
| audio (mp3, m4a, ogg, opus, flac, wav, …) | `<audio controls>` |
| PDF | the browser's viewer in a frame |
| markdown | safe renderer (text nodes only) + "Source" in the editor |
| code, config, dotfiles (`.env`, `.gitignore`, `Dockerfile`, `Makefile`, …) | CodeMirror 6, syntax for 25 languages, edit + save |
| archives: RAR (1.5-7), zip, tar, tar.gz | member list (unsafe names flagged) + "Extract here" |
| everything else | hex dump, paged 4 KiB at a time |

Every file can be downloaded (`Content-Disposition: attachment`, the exact
bytes). With `--files-write`: new file, new folder, upload (drag and drop or
picker, streamed, progress bar), rename, move, copy, extract, edit + save,
delete.

Safety rules (all in `crates/galion-files`, tested):

- Paths are relative to DIR. `..`, `.`, NUL, backslashes and symlinks that
  resolve outside DIR are refused (400/404).
- Read-only unless `--files-write`. Writes also need the header
  `X-Galion-Files: 1` and a same-host `Origin` (blocks cross-site forms).
- Delete never erases: the item moves to `DIR/.trash/<stamp>-<name>`.
  Saving an edit first copies the old file to `DIR/.trash/edits/<stamp>-<name>.bak`.
  Saves check a version token (mtime + size + CRC-32; 409 when someone else
  changed it).
- Nothing overwrites: rename/move refuse an existing name (409); copy,
  upload and extract pick `name (1).ext`.
- Failed uploads and failed extracts are kept as `*.off`, not deleted.
- Archive members with `..`, absolute paths, links or devices are skipped;
  total output is capped by `--files-extract-max` (default 8 GiB).
- Raw bytes go out with `Content-Security-Policy: sandbox`, html/js/svg
  source as `text/plain`, `nosniff`: a file can never run as a console page.

Flags: `--files DIR`, `--files-write`, `--files-max-upload N` (default
2 GiB), `--files-extract-max N`, `--files-ffmpeg PATH` (optional),
`--files-cache DIR` (poster cache, must be outside DIR).

API (all under the gate; `path` is relative and percent-encoded): GET
`/api/files/{list,info,text,hex,archive,kinds,raw,thumb}`, POST
`/api/files/{write,mkdir,rename,move,copy,trash,extract}` (JSON), PUT
`/api/files/upload?dir=&name=` (raw body).

## Research (GitHub, 2026-11-01 CET; licences read from each LICENSE file)

| Project | Licence | Stars | Last push | Use |
| --- | --- | --- | --- | --- |
| codemirror/dev (CodeMirror 6) | MIT | 7.8k | 2026-04 | **picked**: editor, 0.30 MB gzip, built for touch |
| microsoft/monaco-editor 0.57 | MIT | 46.9k | 2026 | rejected: ~2.5 MB core + multi-MB workers; README: mobile browsers not supported |
| bitplane/rars (crate `rars` 0.12.3) | Apache-2.0 | 90 | 2026-10-26 | **picked**: pure-Rust RAR 1.5-7 read/extract |
| muja/unrar.rs | crate MIT/Apache, but bundles the C++ UnRAR source under the proprietary "UnRAR licence" | 340+ | | rejected (not on our allowlist, C++) |
| OSSystems/compress-tools-rs | MIT OR Apache-2.0 | 88 | | rejected: needs system libarchive (C) |
| scryer-media/rarpar | GPL-3.0 | | | rejected (GPL) |
| zip-rs/zip2 (crate `zip` 9.2) | MIT | 382 | 2026-10-31 | **picked**, no features: it only reads the zip directory; members are inflated by our own `inflate.rs` on `miniz_oxide` 0.8 without its SIMD feature (flate2 would force SIMD in), CRC-32 checked |
| galion-tar (in tree) | MIT | | | **picked** for tar / tar.gz |
| sigoden/dufs | MIT OR Apache-2.0 | 11.0k | 2026-10-30 | ideas (upload/Range/zip UX); not linked (actix stack) |
| svenstaro/miniserve | MIT | 8.1k | 2026-10-01 | ideas |
| sxyazi/yazi, Canop/broot | MIT | 43.1k / 13.1k | 2026-10 | ideas: preview-by-kind table |
| filebrowser/filebrowser | Apache-2.0 (Go) | 35.9k | | ideas only (Go) |
| mickael-kerjean/filestash | AGPL-3.0 | | | rejected (AGPL) |
| image-rs/image, bojand/infer, syntect, bat | MIT / Apache | | | not needed: the browser decodes images, own magic-byte sniffer, CodeMirror highlights |

ffmpeg/ffprobe are optional host programs (separate processes, 20 s
timeout), never linked.

Real sample: `ai video Instagram.rar` (RAR5, solid, 13 members, 251 MB)
lists in under 1 ms and extracts in about 2.5-9 s; every member is byte
identical to the `files/` folder (test `real_rar_sample`, opt-in with
`GALION_FILES_SAMPLE_RAR` + `GALION_FILES_SAMPLE_DIR`).

## Tests

- `cargo test -p galion-files`: type table, names, jail (traversal +
  symlink escape), read-only, edit backup + conflict, no-overwrite, trash,
  upload cap → `.off`, zip/tar extraction with hostile members, stored +
  deflated zip with a CRC failure, gzip multi-member, real RAR and real zip
  (opt-in: `GALION_FILES_SAMPLE_RAR`/`_DIR`, `GALION_FILES_SAMPLE_ZIP`).
- `cargo test -p console-static --test files_api`: gate first (401 unpaired,
  foreign host), page + editor bundle, Range 206/416, safe raw headers,
  traversal spellings, read-only 403, write guard, full write round trip,
  posters with host ffmpeg.

## Turning it on (each step needs Slavik's yes; nothing done yet)

1. Pick the folder to share on the VPS (for example a new
   `/srv/galion/files`), and whether the console may write there.
2. Install ffmpeg on the VPS only if posters are wanted (optional).
3. Add to the console unit's ExecStart: `--files /srv/galion/files`
   (+ `--files-write`, `--files-ffmpeg /usr/bin/ffmpeg`,
   `--files-cache /var/cache/galion-console/thumbs`), build and install the
   binary, restart the console unit.
4. Check `/files` from the paired phone.
