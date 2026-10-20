# Obsidian Open Mind: style guide

Obsidian Open Mind, made by Galion. Brand pack version 1.0.0, written 2026-10-18 (Europe/Zagreb, +02:00).
Released under the MIT License, Copyright (c) 3000 Galion / galion-studio (see [../LICENSE](../LICENSE)).

## 1. Name

| Form | Use |
|---|---|
| **Obsidian Open Mind** | the project name. First use on every page, post, README and legal line. |
| **Obsidian Open Mind, made by Galion** | the full brand line: banners, title slides, the site header, the logo lockup. |
| **Open Mind** | short form, only after the full name has appeared once in the same text. |
| `obsidian-open-mind`, `openmind-*` | code only: the repository name and crate prefixes. Never in prose. |

- "Galion" is the maker. Write "made by Galion" in lower case except the G.
- Do not use the older names (Obsidian Open Mind Stack, OpenMind, Open-Mind, Galleon or Galion stack) in new material.
- Obsidian Open Mind is not affiliated with the Obsidian note-taking app. Never borrow that app's logo, purple, or plugin language.

## 2. Voice and tone

Taken from the earlier Galion release kit (`release/brand/BRAND.md`) and kept:

- **Plain.** Short sentences, active voice, the reader is a competent developer.
- **Factual.** Present tense only for what exists. Anything designed but not built says **planned**, **scaffold** or **stub**.
- **No hype.** No "blazing", "world-class", "10x", no invented benchmarks or user counts.
- **No money promises.** No earnings claims and no real-money features.
- **Calm about hardware.** A CPU-only box is a first-class citizen, not a degraded one.
- **Timestamps** are Europe/Zagreb with an explicit offset (`2026-10-18T20:49:11+02:00`), never `Z`.

Approved taglines (pick one per surface): "Rust on CPU. Providers are just rows." (banner) and "The CPU owns the loop and the keys."

## 3. Colour

Obsidian: deep volcanic black glass with one restrained accent. Hexes are reused from the earlier release kit; machine-readable copy in [palette.json](palette.json).

| Token | Hex | Use |
|---|---|---|
| Ink | `#0B0F14` | main dark background; text on light |
| Surface | `#131A22` | panels and code blocks on dark |
| Line | `#24303C` | hairlines on dark; brightest logo facet |
| Text | `#E6EDF3` | body text on dark (16.27:1 on Ink) |
| Mist | `#A7B6C4` | muted text on dark (9.27:1 on Ink) |
| Slate | `#4A7C99` | logo outline (4.24:1 on Ink, 4.29:1 on Paper) |
| Sky | `#58C4FF` | **the** accent on dark: the open ring, links, focus (9.84:1 on Ink) |
| Ocean | `#0B5FA5` | accent on light: links and buttons (6.23:1 on Paper) |
| Paper | `#F7F9FB` | light background |
| Paper surface | `#EEF2F6` | panels on light |
| Paper muted | `#3F4C59` | muted text on light (8.33:1 on Paper) |

Rules: about 90% Ink/Surface, text colours for reading, Sky for at most one or two elements per view. No gradients on text. No purple or pink.

## 4. Typography

Both fonts are open-licensed (SIL Open Font License 1.1) and on Google Fonts. Both are already used by Galion's studio app (`galion-src/galion-studio-app`).

| Role | Font | Fallback stack |
|---|---|---|
| Headings, body, wordmark | [Inter](https://fonts.google.com/specimen/Inter) 400/500/700 | `'Helvetica Neue', Arial, 'Liberation Sans', system-ui, sans-serif` |
| Code, terminals, labels | [Geist Mono](https://fonts.google.com/specimen/Geist+Mono) 400 | `'JetBrains Mono', ui-monospace, Menlo, Consolas, 'DejaVu Sans Mono', monospace` |

The wordmark is "Obsidian Open Mind" in Inter Bold, tracking -0.5 at 46 px; "made by Galion" in Inter Medium at 20 px in Mist (dark) or Paper muted (light). The SVG logos use live text with that fallback stack so the wording stays exact; the PNG exports were rendered with Inter installed.

## 5. Logo

The mark is a hexagonal shard of obsidian cut into six facets, with an open Sky ring around a centre dot: the mind is open, and the loop at the centre belongs to the CPU box.

| File | Use |
|---|---|
| [logo-mark.svg](logo-mark.svg) / [logo-mark-512.png](logo-mark-512.png) | the mark alone: avatars, app icons, small spaces |
| [logo-full-light.svg](logo-full-light.svg) / [.png](logo-full-light.png) | lockup on light backgrounds |
| [logo-full-dark.svg](logo-full-dark.svg) / [.png](logo-full-dark.png) | lockup on dark backgrounds |
| [logo-mono.svg](logo-mono.svg) / [logo-mono-512.png](logo-mono-512.png) | one colour: print, stamps, embossing. Ink on light; recolour the whole mark to Paper on dark. |
| [icons/favicon.svg](icons/favicon.svg), `icons/favicon-16/32/180/512.png` | browser and touch icons (heavier strokes for small sizes) |
| [banner.svg](banner.svg) / [banner.png](banner.png) | 1280x640 repo header and GitHub social preview |

- **Clear space:** keep at least half the mark's width empty on every side (for the 128 px mark: 64 px). In the lockup, measure from the mark.
- **Minimum size:** mark 16 px (use `favicon.svg` below 48 px); lockup 160 px wide.
- **Do:** use the files as they are; place the mark on Ink, Surface or Paper; keep the ring gap at the upper right.
- **Don't:** recolour the ring to anything but Sky (or the mono colour), stretch, rotate, add shadows or glows, outline the text, put the logo on busy photos or mid-grey, or retype the wordmark in another font.

## 6. Planned brand element: Web3 license key

A Web3-based license key system is **planned** and is meant to apply to everything Obsidian Open Mind ships. It is **not implemented**, and its design (chain, key format, how it relates to the MIT License) is undecided. When it exists it will get its own mark derived from the hexagon. Until then, do not show any key, token or chain artwork, and do not describe it as available.
