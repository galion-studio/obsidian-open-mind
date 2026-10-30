#!/bin/sh
# SPDX-License-Identifier: MIT
# Copyright (c) 3000 Galion / galion-studio
# usage: apply-v2.sh <live one/index.html> <out index.html>
# Builds the Galion One page with the V2 skin for one.galion.app only:
#  - before </head>: a tiny inline script that adds class "g1v2" on <html> when
#    location.hostname is one.galion.app (it runs before first paint) and sets theme-color,
#    then v2.css, where every rule is scoped under html.g1v2;
#  - after <body>: the sky element (display:none unless html.g1v2);
#  - applyLook() keeps the V2 theme colour on one.galion.app, and keeps the old colours elsewhere.
# On galion.app / www.galion.app nothing matches, so the page renders as before.
# The loading screen still comes from the galion-one binary (same words, same icon).
set -eu
in="$1"; out="$2"; here=$(dirname "$0")
grep -q 'id="g1v2"' "$in" && { echo "already V2: $in" >&2; exit 2; }
[ "$(grep -c '</head>' "$in")" = 1 ] && [ "$(grep -c '^<body>$' "$in")" = 1 ] || { echo "unexpected page shape" >&2; exit 3; }
old='  if (meta) meta.content = next === "dark" ? "#071018" : "#ffffff";'
[ "$(grep -cxF "$old" "$in")" = 1 ] || { echo "applyLook line not found" >&2; exit 3; }
awk -v css="$here/v2.css" -v old="$old" '
  $0 == "</head>" {
    print "<script>try{if(location.hostname===\"one.galion.app\"){document.documentElement.classList.add(\"g1v2\");var g1m=document.querySelector(\x27meta[name=\"theme-color\"]\x27);if(g1m)g1m.content=\"#05060f\";}}catch(e){}</script>"
    print "<style id=\"g1v2\">"; while ((getline l < css) > 0) print l; print "</style>"
  }
  $0 == old { print "  if (meta) meta.content = document.documentElement.classList.contains(\"g1v2\") ? \"#05060f\" : (next === \"dark\" ? \"#071018\" : \"#ffffff\");"; next }
  { print }
  $0 == "<body>" { print "<div class=\"g1-sky\" aria-hidden=\"true\"><i></i><i></i><i></i></div>" }
' "$in" > "$out"
grep -q 'id="g1v2"' "$out" && grep -q 'class="g1-sky"' "$out" && grep -q 'classList.contains("g1v2")' "$out" || { echo "apply failed" >&2; exit 4; }
echo "ok: $out ($(wc -c < "$out") B)"
