<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Files editor bundle (CodeMirror 6)

`cm6.min.js` is CodeMirror 6 (MIT, Marijn Haverbeke, Adrian Heine and others,
https://github.com/codemirror/dev) bundled with the languages the
galion-files type table names. It is served by `console-static --files` at
`/files/editor.js` and compiled into the binary (`include_bytes!`), so the
console never loads code from a CDN.

- Size: 863,854 bytes minified, 299,202 bytes gzip (Monaco 0.57: about 2.5 MB
  core plus multi-MB workers, and Monaco says mobile browsers are unsupported).
- Global: `GalionEditor.open(parent, text, lang, {readOnly, onSave, onChange})`
  returns `{view, text(), setReadOnly(b), destroy()}`; `GalionEditor.languages`
  lists the language keys. Ctrl/Cmd-S calls `onSave`.
- Languages: rust, javascript/typescript/jsx/tsx, json, css, html, xml,
  markdown, python, sql, yaml, toml, shell, ini/properties, dockerfile, nginx,
  go, c/cpp/java/csharp/kotlin (clike), diff, lua, ruby.

Rebuild (on the box, never on the VPS; needs node + npm):

```sh
mkdir /tmp/cm-build && cp entry.js package.json /tmp/cm-build/ && cd /tmp/cm-build
npm install
npx esbuild entry.js --bundle --minify --format=iife --global-name=GalionEditor \
  --legal-comments=none --outfile=cm6.min.js
cp cm6.min.js <this folder>/
```

Licences: every package in the lock file is MIT (checked 2026-11-01 CET).
