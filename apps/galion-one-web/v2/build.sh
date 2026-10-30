#!/bin/sh
# Builds index-v2.html (one file: inline CSS, markup, worklet source, script). galion-one injects the loading screen after the literal <body>.
set -eu
cd "$(dirname "$0")"
{
printf '%s\n' '<!doctype html>' '<html lang="en" class="g1v2">' '<head>' '<meta charset="utf-8">' '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">' '<meta name="theme-color" content="#05060f">' '<meta name="color-scheme" content="dark">' '<title>Galion One</title>'
printf '%s\n' '<style>'; cat style.css; printf '%s\n' '</style>' '</head>' '<body>'
cat body.html
printf '%s\n' '<script type="text/plain" id="g1-worklet">'; cat mic-worklet.js; printf '%s\n' '</script>' '<script>'; cat app.js; printf '%s\n' '</script>' '</body>' '</html>'
} > index-v2.html
