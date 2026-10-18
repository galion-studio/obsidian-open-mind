# Obsidian Open Mind: brand guide

Made by **Galion** (GitHub: `galion-studio`). Written 2026-10-18T20:50:13+02:00 (Europe/Zagreb, CEST).

This is the short style guide for the public repository. It extends the earlier release kit
(`release/brand/BRAND.md`, `release/site/style.css`) and keeps its colours, contrast table and
voice rules. Machine-readable copies: [`palette.json`](palette.json), [`tokens.css`](tokens.css).

## 1. Name

| Form | Use |
|---|---|
| **Obsidian Open Mind** | First use on any page, in titles, legal lines and alt text. |
| **Open Mind** | Later mentions in the same document. |
| **by Galion** / **made by Galion** | Byline under the name. Galion is the maker; the GitHub org is `galion-studio`. |
| `obsidian-open-mind` | Repository slug and URLs only: `https://github.com/galion-studio/obsidian-open-mind`. |
| `openmind-*` | Crate names and code identifiers. They stay as they are; do not rename them in prose either. |

- Two words, capital O and M: "Open Mind". Never "OpenMind", "Open-Mind" or "OM" in prose.
- The older names "Open Mind Stack" and "Obsidian Open Mind Stack" are retired for public use.
- **Not affiliated with the Obsidian note-taking app.** "Obsidian" here names Galion's internal
  stack (`Stack::cpu_desk()` in `crates/openmind-schema/src/lib.rs` sets `stack = "Obsidian"`).
  Never use that app's logo, colours or icon, and never present Open Mind as a plugin or theme for it.

## 2. Tagline

Primary: **The CPU owns the loop and the keys.** (source: `crates/openmind-schema/src/lib.rs`,
module docs). Alternates from the release kit: "Rust on CPU. Providers are just rows." and
"One contract. Any provider. No GPU needed." Use one tagline per surface.

## 3. Voice and tone

- Plain, short, active. The reader is a competent developer.
- Facts only. Every number comes from a file in the project (binary sizes, test counts) or is not written.
- No hype words ("blazing", "world-class", "10x"). No money talk: real-money access is not approved for this project.
- Say the status: **live**, **staged** (built, not switched on) or **planned** (design only).
- Timestamps are Europe/Zagreb with an explicit offset, for example `2026-10-18T20:50:13+02:00`. Never `Z`.

## 4. Logo

Files (all hand-written SVG; PNGs are exports made with `rsvg-convert`):

| Asset | Dark backgrounds | Light backgrounds |
|---|---|---|
| Mark | [`logo/mark-dark.svg`](logo/mark-dark.svg) | [`logo/mark-light.svg`](logo/mark-light.svg) |
| Mark PNG 512/256/64 | `png/mark-dark-{512,256,64}.png` | `png/mark-light-{512,256,64}.png` |
| Lockup (mark + name + "by Galion") | [`wordmark/lockup-dark.svg`](wordmark/lockup-dark.svg) | [`wordmark/lockup-light.svg`](wordmark/lockup-light.svg) |
| Wordmark (name + "by Galion") | [`wordmark/wordmark-dark.svg`](wordmark/wordmark-dark.svg) | [`wordmark/wordmark-light.svg`](wordmark/wordmark-light.svg) |
| Favicon | [`logo/favicon.svg`](logo/favicon.svg), `png/favicon-32.png`, `png/favicon.ico` (16/32/48) | same (it carries its own ink tile) |
| Banner / social preview 1280x640 | [`banner/banner-1280x640.png`](banner/banner-1280x640.png), [`banner/banner.svg`](banner/banner.svg) | n/a |

Lockup and wordmark files in `wordmark/` have their text converted to outlines (with `usvg`), so they
render without the fonts installed. Editable sources with live text are in `wordmark/src/`; the banner
source is `banner/banner-src.svg`.

**Concept.** A faceted obsidian shard (volcanic black glass) with an open, lit core. The shard is
the box; the core is the loop the CPU owns. The shard shape follows the black faceted mark of
Galion One (`galion-light/one/mark.svg` in the Galion tree); the sky accent comes from the release kit.

**Clear space.** Keep at least 25% of the mark's width empty on every side (for the lockup, 25% of
the mark's height). **Minimum size:** 24 px for the full mark, 16 px only with the favicon variant.

**Do**
- Use the dark variant on Ink/Obsidian, the light variant on Paper or white.
- Scale proportionally; keep the core lit (Sky gradient).
- Use the favicon variant below 48 px.

**Don't**
- Recolour the facets, the rim or the core, or swap the core for another colour.
- Stretch, rotate, outline, add drop shadows or glows beyond what is in the file.
- Put the mark on mid-grey or busy photos, or on the Obsidian notes app's purple.
- Set the name in a font other than Jost, or retype "by Galion" in a different case or font.

## 5. Colour

| Token | Hex | RGB | Role |
|---|---|---|---|
| Obsidian | `#05070A` | 5, 7, 10 | Deepest background: banner, hero, favicon tile |
| Ink | `#0B0F14` | 11, 15, 20 | Primary dark background; body text on paper |
| Surface | `#131A22` | 19, 26, 34 | Raised panels, code blocks |
| Glass | `#1B2633` | 27, 38, 51 | Facet fill in the mark and illustrations |
| Line | `#24303C` | 36, 48, 60 | Hairlines and borders on dark |
| Slate | `#4A7C99` | 74, 124, 153 | Neutral graphic accent (both backgrounds) |
| Mist | `#A7B6C4` | 167, 182, 196 | Muted text on dark |
| Text | `#E6EDF3` | 230, 237, 243 | Body text on dark |
| **Sky** | `#58C4FF` | 88, 196, 255 | **Primary accent** on dark: links, focus, the lit core and rim |
| Ice | `#B8E8FF` | 184, 232, 255 | Specular highlight on dark |
| Ocean | `#0B5FA5` | 11, 95, 165 | Accent on light: links, buttons |
| Paper | `#F7F9FB` | 247, 249, 251 | Light background |
| Muted paper | `#3F4C59` | 63, 76, 89 | Muted text on light |

WCAG 2.1 contrast, computed from the sRGB values: Text on Ink 16.27:1, Text on Obsidian 17.07:1,
Mist on Ink 9.27:1, Sky on Ink 9.84:1, Sky on Obsidian 10.33:1, Ice on Ink 14.67:1, Ink on Paper
18.21:1, Ocean on Paper 6.23:1. **Sky on Paper is 1.85:1: never use Sky for text on light
backgrounds**; use Ocean.

Where it comes from: Ink, Paper, Slate, Sky, Ocean, Mist, Surface and Text are unchanged from the
release kit (`release/brand/BRAND.md`). Sky also sits next to the blue accents of the Galion console
(`#007acc` / `#569cd6` in the console's `styles.css`). Obsidian, Glass and Ice are new and only
extend the dark end and the highlight.

## 6. Typography

All three families are open-licensed under the **SIL Open Font License 1.1** and available from Google Fonts.

| Role | Family | Weights | Link |
|---|---|---|---|
| Display / name | **Jost** | 500 | https://fonts.google.com/specimen/Jost |
| Text | **IBM Plex Sans** | 400, 500, 600 | https://fonts.google.com/specimen/IBM+Plex+Sans |
| Code, labels, "BY GALION" | **IBM Plex Mono** | 400, 500 | https://fonts.google.com/specimen/IBM+Plex+Mono |

Jost is the font already shipped with Galion One (`galion-light/one/font/jost.woff2`); IBM Plex Sans and
Mono are the Galion console's fonts. The byline is set in IBM Plex Mono 500, uppercase, letter-spacing
about 0.18 em: `BY GALION`.

## 7. Icons

Feature icons live in [`icons/`](icons/): 48x48 SVG, ink tile (`#0B0F14`, 11 px radius, `#24303C`
border), Sky line art at 2.25 px stroke, round caps and joins, at most one filled facet in Sky.
They read on both GitHub light and dark themes because each carries its own tile. Keep new icons
angular and geometric (facets, diamonds, chips), no gradients inside icons, no text.

## 8. Copyright line

`Copyright (c) 3000 Galion / galion-studio`, as in [`../LICENSE`](../LICENSE). The year 3000 is a
deliberate choice by the maker; use it unchanged wherever the copyright line
appears. It is not a date: real dates (for example the 2026-10-31 release milestone) are written as they are.
