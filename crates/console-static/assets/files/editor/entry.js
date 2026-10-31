// SPDX-License-Identifier: MIT
// Galion console --files editor bundle: CodeMirror 6 (MIT, Marijn Haverbeke and others).
import {EditorView, basicSetup} from "codemirror";
import {EditorState, Compartment} from "@codemirror/state";
import {keymap} from "@codemirror/view";
import {indentWithTab} from "@codemirror/commands";
import {StreamLanguage} from "@codemirror/language";
import {oneDark} from "@codemirror/theme-one-dark";
import {javascript} from "@codemirror/lang-javascript";
import {rust} from "@codemirror/lang-rust";
import {markdown} from "@codemirror/lang-markdown";
import {json} from "@codemirror/lang-json";
import {html} from "@codemirror/lang-html";
import {css} from "@codemirror/lang-css";
import {python} from "@codemirror/lang-python";
import {yaml} from "@codemirror/lang-yaml";
import {xml} from "@codemirror/lang-xml";
import {sql} from "@codemirror/lang-sql";
import {toml} from "@codemirror/legacy-modes/mode/toml";
import {shell} from "@codemirror/legacy-modes/mode/shell";
import {properties} from "@codemirror/legacy-modes/mode/properties";
import {dockerFile} from "@codemirror/legacy-modes/mode/dockerfile";
import {nginx} from "@codemirror/legacy-modes/mode/nginx";
import {go} from "@codemirror/legacy-modes/mode/go";
import {c, cpp, java, kotlin, csharp} from "@codemirror/legacy-modes/mode/clike";
import {diff} from "@codemirror/legacy-modes/mode/diff";
import {lua} from "@codemirror/legacy-modes/mode/lua";
import {ruby} from "@codemirror/legacy-modes/mode/ruby";

const S = (m) => StreamLanguage.define(m);
const LANG = {
  javascript: () => javascript(), typescript: () => javascript({typescript: true}),
  jsx: () => javascript({jsx: true}), tsx: () => javascript({jsx: true, typescript: true}),
  rust: () => rust(), markdown: () => markdown(), json: () => json(), html: () => html(),
  css: () => css(), python: () => python(), yaml: () => yaml(), xml: () => xml(), sql: () => sql(),
  toml: () => S(toml), shell: () => S(shell), ini: () => S(properties), dockerfile: () => S(dockerFile),
  nginx: () => S(nginx), go: () => S(go), c: () => S(c), cpp: () => S(cpp), java: () => S(java),
  kotlin: () => S(kotlin), csharp: () => S(csharp), diff: () => S(diff), lua: () => S(lua), ruby: () => S(ruby),
};

export function open(parent, text, lang, opts) {
  opts = opts || {};
  const langC = new Compartment(), roC = new Compartment();
  const mk = LANG[lang];
  const ext = [
    basicSetup, oneDark, keymap.of([indentWithTab,
      {key: "Mod-s", preventDefault: true, run: () => { opts.onSave && opts.onSave(); return true; }}]),
    EditorView.lineWrapping, langC.of(mk ? mk() : []), roC.of(EditorState.readOnly.of(!!opts.readOnly)),
    EditorView.updateListener.of((u) => { if (u.docChanged && opts.onChange) opts.onChange(); }),
  ];
  const view = new EditorView({state: EditorState.create({doc: text, extensions: ext}), parent});
  return {
    view,
    text: () => view.state.doc.toString(),
    setReadOnly: (ro) => view.dispatch({effects: roC.reconfigure(EditorState.readOnly.of(!!ro))}),
    destroy: () => view.destroy(),
  };
}
export const languages = Object.keys(LANG);
