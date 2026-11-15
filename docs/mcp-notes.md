<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Research vault MCP

The OpenMind MCP surface was one stdio server, `openmind-mcp-stack`. Its `tools/list` handler returns a single tool, `stack_status` (`crates/openmind-mcp-stack/src/lib.rs`, `TOOL_NAME` and `tools_list`). That crate is a fixture: it reads no files and opens no socket. This note does not change that contract.

A research vault needs a second server. `openmind-mcp-notes` reads a directory of markdown notes and writes only under `drafts/`.

G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network
Developed and distributed by Galion Labs
Perpetual Open MIT License
Copyright © 2026 Galion Labs. All rights reserved.

Those four lines are `BRAND_LINES` in `crates/openmind-desktop/src/page.rs` and again in `crates/openmind-mcp-notes/src/lib.rs`. The crate `LICENSE` file is a byte copy of the root MIT `LICENSE`.

## Before

`openmind-mcp-stack` `tools/list`:

| Tool | What it does |
| --- | --- |
| `stack_status` | Crate names the process was given, plus one Europe/Zagreb timestamp. No files. |

Absent from that list, and absent from every other crate in this repo at the time of the audit:

| Tool | Why a research vault has it |
| --- | --- |
| `search_notes` | Find a note by words in the title, tags or body. |
| `read_note` | Read one note by its vault path. |
| `list_recent` | See what changed, newest first. |
| `search_tags` | Find notes that carry a tag. |
| `backlinks` | See which notes wikilink a note. |
| `create_draft_note` | Capture a new note without touching published ones. |
| `link_notes` | Record a wikilink without editing a published note. |

These usual companions are still absent after this change. They are named so the gap stays visible:

| Tool | Status |
| --- | --- |
| `list_notes` | Not added. `search_notes` and `list_recent` cover finding and the recent slice. |
| `list_tags` | Not added. `search_tags` looks up one tag. |
| append or patch of a published note | Not added. Writes stay under `drafts/`. |
| `move_note` | Not added. |
| `delete_note` | Not added. The server has no remove path. |

## After

`openmind-mcp-stack` is unchanged: `tools/list` is still only `stack_status`.

`openmind-mcp-notes` `tools/list`, in this order (`TOOL_NAMES`):

| Tool | Writes? |
| --- | --- |
| `search_notes` | no |
| `read_note` | no |
| `list_recent` | no |
| `search_tags` | no |
| `backlinks` | no |
| `create_draft_note` | new file under `drafts/` only |
| `link_notes` | appends to a note already under `drafts/`, or creates `drafts/links/<name>.md` |

## Vault rules

The vault is the first argument, or `OPENMIND_NOTES_VAULT` when there is no argument.

```
openmind-mcp-notes /path/to/vault
```

The process speaks newline-delimited JSON-RPC on stdin and stdout. It does not bind a port.

Notes are `*.md` files. A leading `---` block may set `title`, `tags` (a `[a, b]` list or a `- item` list), `created` and `updated`. Tags also come from `#tag` in the body. A fenced code block is skipped for tags and wikilinks. Wikilinks are `[[target]]`, `[[target|alias]]` or `[[target#heading]]`.

Search is an ASCII case-insensitive substring. `score` is a count: 3 for each hit in the title, 2 for each hit in a tag, 1 for each hit in the body. `matched` is how many notes scored above zero before `limit`. `limit` defaults to 20 and cannot be above 50.

`list_recent` orders by the `updated` stamp, newest string first, then path. The stamp is the frontmatter `updated` when that value is Europe/Zagreb RFC 3339 (`+01:00` or `+02:00`). Otherwise it is the file mtime rendered by `zagreb-time`. A stamp ending in `Z` is not kept.

`read_note` takes a vault-relative `.md` path. `..`, a leading slash, a hidden component, a symlink and a non-markdown path are refused. The error text is `path refused` or `note not found`. The response does not include a path outside the vault.

## Drafts

`create_draft_note` builds `drafts/<slug>.md`. The slug is the title folded to ASCII letters and digits. If that file already exists, the server tries `drafts/<slug>-2.md` and so on, up to 50 names. It uses create-new, so an existing file is not replaced. The body and tags are whatever the caller sent, after the title and tag checks.

`link_notes` resolves `from` and `to` by path or by a unique title.

- When `from` is a note under `drafts/`, the server appends `[[to]]` to that draft. A second call with the same target does not append again.
- Otherwise it writes a new proposal under `drafts/links/` whose body contains both wikilinks. A second call returns the same proposal with `wrote: false`.

Published files are not opened for writing. A title that looks like a path, such as `../../etc/passwd`, still lands under `drafts/` because the slug drops the dots.

New draft stamps come from `zagreb-time`. The binary uses the live Zagreb clock. Tests pass a fixed stamp.

## What this server does not do

There is no page and no tone. The desktop reference pitch stays `REFERENCE_HZ` (432) in `crates/openmind-desktop/src/sound.rs`.

There is no socket, no process spawn, and no delete.
