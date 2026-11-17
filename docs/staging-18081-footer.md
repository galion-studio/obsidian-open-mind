# Staging face on :18081: footer branding

Written 2026-11-17 (Europe/Zagreb, +01:00). Swarm 11, agent B3.

## What is wrong

The Obsidian Open Mind face served on Titan `127.0.0.1:18081` ends with a footer
that reads:

> Galion OS · Obsidian Open Mind, made by Galion · Open perpetual MIT ·
> G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network · Maciej Grajczyk

Its "Licence" row also reads "Open perpetual MIT · G.A.L.I.O.N.".

The footer must carry exactly the four branding lines, the same as
`BRAND_LINES` in `crates/openmind-desktop/src/page.rs`:

1. G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network
2. Developed and distributed by Galion Labs
3. Perpetual Open MIT License
4. Copyright © 2026 Galion Labs. All rights reserved.

## Where the page lives

The :18081 page is not built from this repo. It is a hand-made folder on Titan
(`/srv/Galion/projects/galion-os-console-staging/public/index.html`, served by
its own `serve.py`). That folder is not a git checkout, and no branch of this
repo or the other Galion repos on the laptop holds the stale footer text.

## The fix

`brand/footer.html` holds the four lines as ready HTML. Applying it on Titan:

1. In `public/index.html`, replace the first `<p>` inside `<footer class="foot">`
   with the four `<p class="brand">` lines from `brand/footer.html`. Keep the
   second `<p class="foot-m">` (staging note) as it is.
2. Change the "Licence" row value to `Perpetual Open MIT License`.
3. Check: `curl -s http://127.0.0.1:18081/ | grep -c 'class="brand"'` prints `4`,
   and `grep -c 'Open perpetual MIT'` prints `0`.

`serve.py` serves the file from disk, so no restart is needed for a static
change. Editing a file on Titan needs Mate's yes on a card; this PR only adds
the canonical lines and this note.

`scripts/check-brand-footer.sh` checks that `brand/footer.html` matches
`BRAND_LINES` line for line.
