#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# usage: apply-v2.sh <live one/index.html> <out index.html>
# Builds the Galion One V2 page: inlines v2.css before </head>, adds the sky after <body>,
# and sets the browser theme colour to the V2 night sky. Nothing else in the page changes
# (same DOM, same JS, same words). The loading screen comes from the galion-one binary as before.
set -eu
in="$1"; out="$2"; here=$(dirname "$0")
grep -q 'id="g1v2"' "$in" && { echo "already V2: $in" >&2; exit 2; }
[ "$(grep -c '</head>' "$in")" = 1 ] && [ "$(grep -c '^<body>$' "$in")" = 1 ] || { echo "unexpected page shape" >&2; exit 3; }
awk -v css="$here/v2.css" '
  $0 == "</head>" { print "<style id=\"g1v2\">"; while ((getline l < css) > 0) print l; print "</style>" }
  { print }
  $0 == "<body>" { print "<div class=\"g1-sky\" aria-hidden=\"true\"><i></i><i></i><i></i></div>" }
' "$in" | sed \
  -e 's|<meta name="theme-color" content="#C4553A">|<meta name="theme-color" content="#05060f">|' \
  -e 's|meta.content=look==="dark"?"#C4553A":"#C4553A"|meta.content="#05060f"|' \
  -e 's|meta.content = next === "dark" ? "#071018" : "#ffffff"|meta.content = "#05060f"|' > "$out"
grep -q 'id="g1v2"' "$out" && grep -q 'class="g1-sky"' "$out" || { echo "apply failed" >&2; exit 4; }
echo "ok: $out ($(wc -c < "$out") B)"
