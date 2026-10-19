<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Persona mirror

- DESK-ID: openmind.PERSONA-MIRROR
- Written: 2026-10-19T21:06:19+02:00
- Repo: /opt/galion/openmind
- Left: personas/persona-plex-v1.toml
- Right: personas/persona-plex-v1.md
- Scope: min_sentences, max_sentences, and the role prompt. Neither persona file was edited. The voice reference was not opened.

The stamp offset is numeric. It is not a UTC mark.

## Result

Match: yes.

| item | toml | md | match |
| --- | --- | --- | --- |
| min_sentences | 1 | one, the lower end of "one or two" | yes |
| max_sentences | 2 | two, the upper end of "one or two" | yes |
| role prompt | 7 lines, 646 bytes | 7 lines, 646 bytes, same text | yes |

## min_sentences

In the toml reply table, min_sentences is 1.
The md file has no key named min_sentences.
The md length line calls the official rule one or two short sentences.
The shared role prompt says: Keep it to one or two short sentences.
The lower end is one. That is the same as 1.
min_sentences matches.

## max_sentences

In the toml reply table, max_sentences is 2.
The md file has no key named max_sentences.
The md length line calls the official rule one or two short sentences.
The upper end is two. That is the same as 2.
max_sentences matches.

The toml rule string is "one or two short sentences".
The toml unit string is "short sentences".
Both agree with the md length line.
Old source counts (four, two, one) are the replaced counts, in both files. They are not the official bound.

## Role prompt

The toml role_prompt value and the md role prompt block are the same bytes.
A line-by-line compare found no difference.
Both hold these 7 lines:

You are Persona Plex, the voice of Galion One in the Obsidian Open Mind stack. The person is speaking to you. You are not the admin desk.
Start in a few words, warm and close, then one short line. Keep it to one or two short sentences.
Plain English. No greeting unless they greeted you. Do not say you are a model or a system. You are not a copy of another model.
No lists, no headings, no markdown, no paths, no code. Say the answer, not the path.
If you were not given a live number, say you do not have it.
You do not write files, run commands, move money, or spawn anyone.
You keep the Obsidian Hearth: knowledge, prosperity, and integrity.

## Not done

- personas/persona-plex-v1.toml was not edited.
- personas/persona-plex-v1.md was not edited.
- The voice reference was not opened.
- No commit. No push.
