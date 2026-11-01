<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Hermes skill: shared files

Hermes can browse, read, and (when allowed) organise one shared folder, the
same folder and the same Rust jail (`galion-files`) as the console's `/files`
explorer. The tools live in `crates/hermes-tools/src/files.rs` and are wired
into `hermes-gateway` (feature `coding`). They are off unless configured.

## Turning it on (needs Slavik's yes; nothing is on yet)

| env | meaning | default |
|---|---|---|
| `HERMES_RS_FILES_ROOT` | absolute path of the shared folder; unset = tools off | unset |
| `HERMES_RS_FILES_WRITE` | `1` adds the write tools | `0` (read-only) |
| `HERMES_RS_FILES_AUDIT` | JSON Lines audit log, outside the root | `<hermes home>/files-audit.jsonl` |
| `HERMES_RS_FILES_MAX_READ` | bytes per `files_read` | 1048576 |
| `HERMES_RS_FILES_EXTRACT_MAX_MB` | total unpacked size per extract | 4096 |
| `HERMES_RS_FILES_MAX_CALLS` | file tool calls per session | 400 |

Refused at startup: a relative path, `/` or a system folder, and any folder
holding `SOUL.md` or `config.yaml` (hermes's own home stays out of reach).
SOUL.md and config.yaml are not changed by this work.

## Tools

| tool | does | args |
|---|---|---|
| `files_list` | folder listing: name, kind, size, mtime (Europe/Zagreb), hidden | `path`, `hidden` |
| `files_read` | text/code as text + `version`; other files as a hex page; `mode=archive` lists members | `path`, `mode` (auto/text/hex/archive), `offset`, `max_bytes` |
| `files_info` | kind, MIME, size, mtime, mode, preview type | `path` |
| `files_kinds` | the whole extension table (below) | none |
| `files_write` * | save text; edits need `version` from `files_read`; `create=true` for new files | `path`, `content`, `version`, `create` |
| `files_mkdir` * | new empty folder | `dir`, `name` |
| `files_rename` * | rename in place | `path`, `new_name` |
| `files_move` * | move, or copy with `copy=true` | `path`, `dest_dir`, `copy` |
| `files_extract` * | unpack .rar/.zip/.tar/.tar.gz into a fresh folder | `path`, `dest_dir` |
| `files_trash` * | move into `.trash/` (the only "delete") | `path` |

\* only with `HERMES_RS_FILES_WRITE=1`.

## Behaviour (how Hermes should use them)

1. Look before acting: `files_list`, then `files_info` or `files_read`.
   Never guess a path; paths are relative to the shared root, `/` separated.
2. Read text with `files_read`. Binary files come back as hex; describe
   media from `files_info` (kind, size, and for videos what the console
   shows) instead of dumping bytes. Tell the human the console link
   `/files#<path>` to watch or view it.
3. Editing: always `files_read` first, change the text, then `files_write`
   with the `version` you got. On a conflict, read again and redo the change;
   never force. The old copy is kept in `.trash/edits/`; say so.
4. Never delete. To get rid of something use `files_trash` and tell the
   human where it went. Never empty `.trash/`.
5. Rename, move and copy never overwrite. If a name is taken, pick a new
   one (or ask) instead of trashing the old file.
6. Archives: list with `files_read mode=archive` first; extract only when
   asked. Report skipped members (unsafe names, encrypted, unsupported).
7. Secrets: `files_read` refuses `.env*`, private keys, `.pem`, `.key`,
   `.netrc`, credential files. Do not try to work around it; ask a human.
   Never paste tokens or keys into chat even if a file you read holds one.
8. Big files: page with `offset` / `max_bytes`; do not read a whole video.
9. Every call is in the audit log (contents never logged). Be brief and
   purposeful; there is a per-session call cap.

## Extension table (from `galion-files/src/kinds.rs`; `files_kinds` returns it live)

| kind | preview | extensions |
|---|---|---|
| image | image | png, jpg, jpeg, jfif, gif, webp, avif, bmp, ico, svg, apng, tif, tiff, heic, heif |
| video | video player | mp4, m4v, mov, webm, mkv, avi, ogv, 3gp, mts |
| code | editor | ts, rs, toml, lock, json, jsonl, ndjson, json5, yaml, yml, js, mjs, cjs, jsx, tsx, html, htm, css, scss, xml, plist, py, sh, bash, zsh, fish, sql, ini, cfg, conf, env, properties, service, timer, socket, go, c, h, cc, cpp, hpp, java, kt, kts, cs, lua, rb, diff, patch, nginx, swift, php, pl, vue, svelte, proto, graphql, tf, nix, pem, pub |
| audio | audio player | mp3, m4a, aac, wav, flac, ogg, oga, opus, weba |
| pdf | PDF viewer | pdf |
| markdown | rendered + editor | md, mdx |
| text | editor | txt, log, csv, tsv, rst, adoc, srt, vtt, bak, off |
| document | hex | doc, xls, ppt, epub, rtf |
| archive | member list + extract | zip, rar, cbz, cbr, jar, tar, tgz |
| packed | hex | gz, 7z, xz, bz2, zst, iso, deb, rpm, dmg |
| font | hex | ttf, otf, woff, woff2 |
| binary | hex | wasm, so, bin, gguf, safetensors, onnx, db, sqlite, sqlite3 |

Dotfiles and special names (`.gitignore`, `.env`, `Dockerfile`, `Makefile`,
`Cargo.lock`, ...) are known by name; anything else is sniffed by its first
bytes (magic numbers, else "looks like text" => editor, else hex).

## Safety notes

- In-process: the file tools never start a process and do not run in
  openmind-sandbox; the jail is `galion-files` (canonical root, no `..`, no
  absolute paths, symlinks may not leave the root, `.trash` is read-only).
- The coding tools (`fs_read`, `fs_write`, `git`, `cargo`) are separate and
  unchanged; they keep their sandbox and worktree.

## Tests

`cargo test -p hermes-tools --test files_tools`: config refusals (off,
relative, `/`, folder with SOUL.md), read-only by default (no write tools
offered), listing hides dotfiles, text/hex/secret refusal, traversal and
unknown-argument refusals, audit lines without contents, full write round
trip (version check, stale version refused, create, mkdir, rename/move
without overwrite, copy, trash, zip list + extract).
