#!/usr/bin/env bash
# Check brand/footer.html carries exactly the four branding lines, in order,
# matching BRAND_LINES in crates/openmind-desktop/src/page.rs. MIT.
set -euo pipefail
cd "$(dirname "$0")/.."
want=(
  "G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network"
  "Developed and distributed by Galion Labs"
  "Perpetual Open MIT License"
  "Copyright © 2026 Galion Labs. All rights reserved."
)
mapfile -t got < <(sed -n 's/^<p class="brand">\(.*\)<\/p>$/\1/p' brand/footer.html)
if [ "${#got[@]}" -ne 4 ]; then
  echo "FAIL: brand/footer.html has ${#got[@]} brand lines, want 4" >&2; exit 1
fi
for i in 0 1 2 3; do
  if [ "${got[$i]}" != "${want[$i]}" ]; then
    echo "FAIL: line $((i+1)): got '${got[$i]}' want '${want[$i]}'" >&2; exit 1
  fi
done
# page.rs writes the dash and copyright sign as \u{2014} and \u{00A9}.
rs=crates/openmind-desktop/src/page.rs
grep -qF '"G.A.L.I.O.N. \u{2014} General Autonomous Local Intelligence Open Network"' "$rs"
grep -qF '"Developed and distributed by Galion Labs"' "$rs"
grep -qF '"Perpetual Open MIT License"' "$rs"
grep -qF '"Copyright \u{00A9} 2026 Galion Labs. All rights reserved."' "$rs"
if grep -qiE 'open perpetual mit|made by galion|grajczyk' brand/footer.html; then
  echo "FAIL: stale footer text in brand/footer.html" >&2; exit 1
fi
echo "OK: brand/footer.html has the four branding lines and matches page.rs"
