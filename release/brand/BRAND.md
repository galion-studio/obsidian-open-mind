# BRAND — Obsidian Open Mind

Written 2026-10-18T19:06:14+02:00 (Europe/Zagreb, CEST). Owner: subagent A "site-brand".
Every claim below carries a source in this form: `file:line`, where the path roots are

- `ROOT` = `/opt/galion/openmind`
- `M` = `ROOT/contrib/grokbot/docs/main.md`
- `A` = `ROOT/contrib/grokbot/docs/study/A-foundation.md`

Line numbers were read at 2026-10-18T19:17:58+02:00 (Europe/Zagreb). The workspace is being
built by another worker during this session and gained crates while this file was being
written (`crates/` held six at that timestamp), so re-read every line number before reusing
one. Sources that only exist in the release kit (the README draft) are cited as
`release/repo/...`.

## 1. Name usage

| Form | When | Notes |
|---|---|---|
| **Obsidian Open Mind** | first use in any document, page, post, README, licence, legal line, or where the name stands alone | the full name, capital O twice, capital M twice |
| **Open Mind** | second and later uses inside the same document | short form only after the full name has appeared once |
| **Open Mind stack** | when talking about the collection of programs, not the project as a whole | acceptable variant |
| `openmind` | the code namespace only: repository, Cargo workspace, crate name prefixes, directory | lowercase, no space, never for prose |

Rules:

- The stance string is `Open Mind`, the stack is `Obsidian`, the studio is `Galion Studio`
  (`ROOT/crates/openmind-schema/src/lib.rs:89-91`).
- Written "Open Mind" as two words. Never "OpenMind", "Open-Mind", "openmind" or "OM" in prose.
- Never write the name in a way that reads as a company: Open Mind is a project and a stance,
  not a product line and not a vendor.

### Relation to the Obsidian stack and Galion Studio

Obsidian Open Mind is the stance and the code of the Obsidian stack built by Galion Studio
(`release/repo/README.md:3-6`; `ROOT/README.md:3`; `ROOT/crates/openmind-schema/src/lib.rs:89-91`,
where `Stack::cpu_desk()` sets `studio = "Galion Studio"`, `stack = "Obsidian"`,
`stance = "Open Mind"`). Galion Studio is the studio; Obsidian is the stack it builds; Open
Mind is the stance the code follows.

### NOT affiliated with the Obsidian notes app

**Obsidian Open Mind is not affiliated with, endorsed by, or connected to the Obsidian
note-taking application (the Markdown notes app), or with any of that project's maintainers.**
The word "Obsidian" here names an internal stack of Galion Studio,
not the notes app. Any page, README or post that uses the name must carry this disclaimer,
or must link to a page that does.

We do not use the notes app's logo, icon, or colour, and we never present Open Mind as a
plugin, add-on, theme or companion to that app. Avoid sentences built to catch that app's
search traffic.

## 2. Tagline options

Five options, each at most 8 words, each traceable. Pick one per surface; do not chain two.

| # | Tagline | Words | Trace |
|---|---|---|---|
| 1 | The CPU owns the loop and the keys. | 8 | `ROOT/crates/openmind-schema/src/lib.rs:3` |
| 2 | Rust on CPU. Providers are just rows. | 7 | `ROOT/crates/openmind-schema/src/lib.rs:49`; `M:30` |
| 3 | A CPU-light foundation for the Obsidian stack. | 7 | `M:30`; `A:26` |
| 4 | One contract. Any provider. No GPU needed. | 7 | `ROOT/crates/openmind-schema/src/lib.rs:51-58`; `A:76` |
| 5 | Small Rust. Your box keeps the keys. | 7 | `ROOT/crates/openmind-schema/src/lib.rs:62`; `A:26` |

Option 5 may only be used where the code is the subject; it is a design statement, not a
security guarantee. No tagline claims a speed multiplier, a user count, a benchmark or a
saving, because none of those is traced in `release/FACTS.md`.

## 3. Voice and tone

- **Plain.** Short sentences. Active voice. The reader is a competent developer.
- **Factual.** Every number is traced to a file and line, or it is not written. No number is
  carried "about" or "approximately" without its source next to it.
- **No hype.** No "blazing", "world-class", "game-changing", "effortless", "10x". If a
  thing is not built yet, the word "planned" goes in the sentence.
- **No promises about money.** No earnings, no payments, no real-money rails, no "no profit
  promises" text beyond this rule. (This line is the only permitted use of the word in this
  directory, per `TEACHER/CHECKS.md` K7.)
- **Present tense only for what exists.** Present tense for code in the workspace; "planned"
  plus a `TODO(verify)` marker for anything named but not yet written.
- **Calm about hardware.** A CPU-only box is a first-class citizen, not a degraded one
  (`ROOT/crates/openmind-schema/src/lib.rs:29-36`). Never apologise for not needing a card.
- **Timestamps.** Every published timestamp is Europe/Zagreb with an explicit offset
  (`M:52-55`). Never server time, never `Z`.

### Do / Don't

| Do write | Don't write |
|---|---|
| "`crates/` holds seven crates today: the contract, the time helper and five programs, one of them a `Cargo.toml` with no source yet. Nothing is published." | "The whole stack ships today." |
| "The processor runs this. The card only runs the model." (`ROOT/crates/openmind-schema/Cargo.toml:6`) | "Saves you money on GPU bills." |
| "A 2026-10-18 snapshot of the pre-workspace binaries put the Rust parts at about 10 MB of RAM and almost no CPU." (`A:26`) | "Uses almost no resources at all." |
| "Adding a site is a new row, not a new program." (`ROOT/crates/openmind-schema/src/lib.rs:49`) | "Add unlimited providers for free." |
| "No Rust toolchain version is pinned in the workspace." (`ROOT/Cargo.toml`, no `rust-version` key) | "Works on every Rust version." |
| "Released under the MIT license." (`M:26`) | "Fully open, do anything you want." |
| "The repository is not published yet." | "Available now on GitHub." |
| "Planned: a `zagreb-time` helper." (now present, `ROOT/crates/zagreb-time/`) | "Includes a time helper." (without saying which state) |

## 4. Colour palette

Six core colours. Roles follow the dark-first site design; the light pair exists so the same
brand works on white paper.

| Token | Hex | Role |
|---|---|---|
| Ink | `#0B0F14` | dark background; body text on paper |
| Paper | `#F7F9FB` | light background |
| Slate | `#4A7C99` | the logo mark; a neutral accent that holds on both backgrounds |
| Sky | `#58C4FF` | accent on dark: links, focus rings, the hero mark |
| Ocean | `#0B5FA5` | accent on light: links, buttons |
| Mist | `#A7B6C4` | muted text on dark |

Supporting tokens used by the contrast pairs: Surface `#131A22` (raised panel, dark),
Surface Paper `#EEF2F6` (raised panel, light), Text `#E6EDF3` (body text on dark),
Muted on paper `#3F4C59`.

### WCAG contrast ratios (computed, not estimated)

Computed with the WCAG 2.1 relative-luminance formula on the sRGB values; the script is an
awk one-liner, no library. AA needs 4.5:1 for normal text, 3:1 for large text and for
non-text graphics.

| Text / mark | On | Ratio | Use | AA |
|---|---|---|---|---|
| `#E6EDF3` | `#0B0F14` | **16.27:1** | body text, dark theme | pass (AAA) |
| `#A7B6C4` | `#0B0F14` | **9.27:1** | muted text, dark theme | pass (AAA) |
| `#58C4FF` | `#0B0F14` | **9.84:1** | links and accent, dark theme | pass (AAA) |
| `#0B0F14` | `#58C4FF` | **9.84:1** | label on an accent button, dark theme | pass (AAA) |
| `#E6EDF3` | `#131A22` | **14.83:1** | body text on a dark panel | pass (AAA) |
| `#A7B6C4` | `#131A22` | **8.45:1** | muted text on a dark panel | pass (AAA) |
| `#0B0F14` | `#F7F9FB` | **18.21:1** | body text, light theme | pass (AAA) |
| `#3F4C59` | `#F7F9FB` | **8.33:1** | muted text, light theme | pass (AAA) |
| `#0B5FA5` | `#F7F9FB` | **6.23:1** | links and accent, light theme | pass (AA) |
| `#FFFFFF` | `#0B5FA5` | **6.57:1** | label on a light-theme button | pass (AA) |
| `#3F4C59` | `#EEF2F6` | **7.81:1** | muted text on a light panel | pass (AAA) |
| `#4A7C99` | `#0B0F14` | **4.24:1** | logo mark on dark (graphic, needs 3:1) | pass |
| `#4A7C99` | `#F7F9FB` | **4.29:1** | logo mark on light (graphic, needs 3:1) | pass |
| `#5E8CA6` | `#0B0F14` | **5.29:1** | logo detail on dark | pass |
| `#5E8CA6` | `#F7F9FB` | **3.44:1** | logo detail on light (graphic) | pass |

Every text pair in the site is drawn from this table.

## 5. Logo

`logo.svg` (both `brand/logo.svg` and the identical `site/logo.svg`) is hand-written SVG,
under 5 KB, no raster, no fonts, no external references, text-free and scalable.

- Concept: a rounded box (the machine) holding a centre dot (the loop the CPU owns), with
  three small squares along the base (the provider rows). "Adding a site is a new row"
  (`ROOT/crates/openmind-schema/src/lib.rs:49`).
- Colour: one mid-luminance slate, `#4A7C99` for the box and dot and `#5E8CA6` for the
  squares. Because the mark sits in the middle of the luminance range it clears 3:1 against
  both `#0B0F14` (4.24:1) and `#F7F9FB` (4.29:1) without a theme switch, a filter or a
  second file. The SVG also sets `color-scheme`-independent explicit fills, so it renders the
  same everywhere, including as a favicon or as an `<img>`.
- Clear space: leave at least 25% of the mark's width on every side. Minimum size 24 px.
- Do not recolour it to a low-contrast tint, stretch it, rotate it, add a drop shadow, or
  place it on a mid-grey between the two background values.
