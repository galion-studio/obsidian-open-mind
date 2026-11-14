// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The control-console document. One HTML page, self-contained.
//!
//! Colours are the Open Mind tokens (Ink, Surface, Sky, Paper). No font is
//! fetched. The seven tones are embedded as `data:` URLs so opening the
//! file needs no server and no network.

use crate::sound::{self, TONES};
use crate::stack::LAYERS;
use std::sync::OnceLock;

/// Content-Security-Policy for the page. Scripts and styles are inline.
/// Connections, frames and form posts are refused.
pub const CSP: &str = "\
default-src 'none'; \
style-src 'unsafe-inline'; \
script-src 'unsafe-inline'; \
media-src data:; \
img-src data:; \
base-uri 'none'; \
form-action 'none'; \
frame-ancestors 'none'";

const CSS: &str = r#"
:root {
  --bg: #0B0F14;
  --bg-deep: #05070A;
  --panel: #131A22;
  --line: #24303C;
  --fg: #E6EDF3;
  --muted: #A7B6C4;
  --accent: #58C4FF;
  --focus: #58C4FF;
  color-scheme: dark;
}
@media (prefers-color-scheme: light) {
  :root {
    --bg: #F7F9FB;
    --bg-deep: #F7F9FB;
    --panel: #EEF2F6;
    --line: #D5DDE5;
    --fg: #0B0F14;
    --muted: #3F4C59;
    --accent: #0B5FA5;
    --focus: #0B5FA5;
    color-scheme: light;
  }
}
* { box-sizing: border-box; }
html, body { margin: 0; height: 100%; }
body {
  background: var(--bg-deep);
  color: var(--fg);
  font: 16px/1.45 "IBM Plex Sans", system-ui, sans-serif;
}
.app {
  min-height: 100%;
  display: grid;
  grid-template-columns: 17rem 1fr;
  grid-template-rows: auto 1fr auto;
}
header, footer {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 16px;
  background: var(--bg);
  border-bottom: 1px solid var(--line);
}
footer {
  border-bottom: 0;
  border-top: 1px solid var(--line);
  color: var(--muted);
  font-size: 13px;
}
header p { margin: 0; }
.kicker {
  color: var(--muted);
  font: 12px/1.3 "IBM Plex Mono", ui-monospace, monospace;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
h1 { font-size: 18px; font-weight: 600; }
.stack {
  padding: 16px;
  border-right: 1px solid var(--line);
  background: var(--bg);
}
.stack h2, .stage h2 {
  margin: 0 0 10px;
  font: 12px/1.3 "IBM Plex Mono", ui-monospace, monospace;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 500;
}
.stack ol { list-style: none; margin: 0; padding: 0; }
.stack li {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 2px 8px;
  padding: 8px 0;
  border-top: 1px solid var(--line);
}
.stack li:first-child { border-top: 0; }
.stack .name { font-weight: 600; }
.stack .role { grid-column: 1 / -1; color: var(--muted); font-size: 13px; }
.stack .state {
  font: 12px/1.3 "IBM Plex Mono", ui-monospace, monospace;
  color: var(--accent);
}
.stage {
  display: grid;
  place-items: center;
  padding: 24px 16px;
}
.presence { width: min(40rem, 100%); }
.presence > p { margin: 0 0 16px; color: var(--muted); }
.prompt {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px;
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 11px;
}
.plus, .voice, #mute {
  position: relative;
  width: 44px;
  height: 44px;
  flex: 0 0 44px;
  display: grid;
  place-items: center;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: var(--bg);
  color: var(--fg);
  padding: 0;
}
.plus input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: pointer;
}
.plus svg, .voice svg { width: 22px; height: 22px; }
#command {
  flex: 1;
  min-width: 0;
  height: 44px;
  border: 0;
  background: transparent;
  color: var(--fg);
  font: inherit;
  padding: 0 8px;
}
#command:focus { outline: none; }
.prompt:focus-within, .plus:focus-within, .voice:focus-visible, #mute:focus-visible {
  outline: 2px solid var(--focus);
  outline-offset: 2px;
}
.voice[aria-pressed="true"] { color: var(--accent); border-color: var(--accent); }
#mute { width: auto; padding: 0 12px; font: inherit; }
.notes { margin: 12px 0 0; color: var(--muted); font-size: 14px; }
.notes ul { margin: 8px 0 0; padding-left: 1.1rem; }
audio { display: none; }
@media (max-width: 720px) {
  .app { grid-template-columns: 1fr; }
  .stack { border-right: 0; border-bottom: 1px solid var(--line); }
  .stage { min-height: 50vh; }
}
"#;

/// Browser script. Names and commands stay in the document. File bytes are
/// not read. The microphone is not opened. Night hours and Mute skip tones.
const SCRIPT: &str = r#"
(function () {
  var files = document.getElementById("files");
  var command = document.getElementById("command");
  var voice = document.getElementById("voice");
  var voiceState = document.getElementById("voice-state");
  var picked = document.getElementById("picked");
  var kept = document.getElementById("kept");
  var mute = document.getElementById("mute");
  function night() {
    var h = new Date().getHours();
    return h >= 22 || h < 7;
  }
  function play(id) {
    if (mute.getAttribute("aria-pressed") === "true" || night()) return;
    var el = document.querySelector("audio[data-tone='" + id + "']");
    if (!el) return;
    try { el.currentTime = 0; } catch (e) {}
    var pending = el.play();
    if (pending && pending.catch) pending.catch(function () {});
  }
  files.addEventListener("change", function () {
    picked.textContent = "";
    var list = files.files;
    var i;
    for (i = 0; i < list.length; i = i + 1) {
      var row = document.createElement("li");
      row.textContent = list[i].name;
      picked.appendChild(row);
    }
    if (list.length) play("hear");
  });
  voice.addEventListener("click", function () {
    var on = voice.getAttribute("aria-pressed") === "true";
    voice.setAttribute("aria-pressed", on ? "false" : "true");
    if (on) {
      voiceState.textContent = "Microphone closed.";
      play("close");
    } else {
      voiceState.textContent = "Voice armed. The microphone stays closed.";
      play("ask");
    }
  });
  command.addEventListener("keydown", function (ev) {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    var text = command.value.replace(/^\s+|\s+$/g, "");
    if (!text) { play("deny"); return; }
    var row = document.createElement("li");
    row.textContent = text;
    kept.appendChild(row);
    command.value = "";
    play("done");
  });
  mute.addEventListener("click", function () {
    var on = mute.getAttribute("aria-pressed") === "true";
    mute.setAttribute("aria-pressed", on ? "false" : "true");
    mute.textContent = on ? "Mute" : "Muted";
  });
})();
"#;

/// The control-console page. Built once.
#[must_use]
pub fn document() -> &'static str {
    static PAGE: OnceLock<String> = OnceLock::new();
    PAGE.get_or_init(render)
}

fn render() -> String {
    let mut html = String::with_capacity(48 * 1024);
    html.push_str(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Obsidian Open Mind</title>\
<meta name=\"description\" content=\"Open Mind control console. Local only.\">\
<meta name=\"color-scheme\" content=\"dark light\">\
<style>",
    );
    html.push_str(CSS);
    html.push_str("</style></head><body><div class=\"app\" data-desktop=\"control-console\">");
    html.push_str(
        "<header><div><p class=\"kicker\">Obsidian Open Mind</p>\
<h1>Control console</h1></div>\
<button type=\"button\" id=\"mute\" aria-pressed=\"false\">Mute</button></header>",
    );
    html.push_str("<section class=\"stack\" aria-label=\"Stack\"><h2>Stack</h2><ol>");
    for layer in LAYERS {
        html.push_str("<li data-layer=\"");
        html.push_str(layer.id);
        html.push_str("\"><span class=\"name\">");
        html.push_str(&esc(layer.name));
        html.push_str("</span><span class=\"state\">");
        html.push_str(layer.state);
        html.push_str("</span><span class=\"role\">");
        html.push_str(&esc(layer.role));
        html.push_str("</span></li>");
    }
    html.push_str("</ol></section>");
    html.push_str(
        "<main class=\"stage\" id=\"main\"><div class=\"presence\">\
<h2>Open Mind</h2>\
<p>Ask from here. The command stays on this page. No model is called.</p>\
<div class=\"prompt\" role=\"group\" aria-label=\"Command\">\
<label class=\"plus\"><input id=\"files\" type=\"file\" multiple aria-label=\"Add files\">",
    );
    html.push_str(PLUS);
    html.push_str(
        "</label><input id=\"command\" type=\"text\" autocomplete=\"off\" \
placeholder=\"Ask Open Mind\" enterkeyhint=\"send\" aria-label=\"Command\">\
<button type=\"button\" id=\"voice\" aria-pressed=\"false\" aria-label=\"Voice\">",
    );
    html.push_str(MIC);
    html.push_str(
        "</button></div>\
<p class=\"notes\" id=\"voice-state\">Microphone closed.</p>\
<div class=\"notes\"><p>Files</p><ul id=\"picked\"></ul></div>\
<div class=\"notes\"><p>Kept on this page</p><ul id=\"kept\"></ul></div>\
</div></main>",
    );
    html.push_str(
        "<footer><span>MIT License. Local only. 432 Hz.</span>\
<span>Copyright (c) 3000 Galion / galion-studio</span></footer>",
    );
    for tone in TONES {
        let bytes = sound::wav(tone.id).expect("tone id");
        let b64 = galion_common::base64::encode(&bytes);
        html.push_str("<audio data-tone=\"");
        html.push_str(tone.id);
        html.push_str("\" src=\"data:audio/wav;base64,");
        html.push_str(&b64);
        html.push_str("\" preload=\"none\"></audio>");
    }
    html.push_str("<script>");
    html.push_str(SCRIPT);
    html.push_str("</script></div></body></html>\n");
    html
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

const PLUS: &str = "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">\
<path d=\"M12 5v14M5 12h14\" fill=\"none\" stroke=\"currentColor\" \
stroke-width=\"1.8\" stroke-linecap=\"round\"/></svg>";

const MIC: &str = "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\">\
<path d=\"M12 4a3 3 0 0 0-3 3v4a3 3 0 0 0 6 0V7a3 3 0 0 0-3-3M8 18a4 4 0 0 0 8 0M12 18v2\" \
fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.6\"/></svg>";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{self, TONES};

    #[test]
    fn prompt_is_plus_then_field_then_voice() {
        let html = document();
        assert!(html.contains("data-desktop=\"control-console\""));
        let plus = html.find("id=\"files\"").unwrap();
        let field = html.find("id=\"command\"").unwrap();
        let voice = html.find("id=\"voice\"").unwrap();
        assert!(plus < field && field < voice);
        let input = &html[plus.saturating_sub(40)..plus + 80];
        assert!(input.contains("type=\"file\""));
        assert!(input.contains("multiple"));
        assert!(!input.contains("accept="));
        assert!(html.contains("aria-label=\"Voice\""));
        assert!(html.contains(">Open Mind</h2>"));
        assert!(html.contains("Control console"));
        assert!(html.contains("MIT License"));
        assert!(html.contains("432 Hz"));
        assert!(!html.contains("border-radius:50%"));
        assert!(!html.contains("border-radius: 50%"));
        assert!(!html.contains("id=\"talk\""));
    }

    #[test]
    fn page_stays_on_the_machine() {
        let html = document();
        for banned in [
            "http://",
            "https://",
            "ws://",
            "wss://",
            "getUserMedia",
            "FileReader",
            "innerHTML",
            "XMLHttpRequest",
            "WebSocket",
            "fetch(",
        ] {
            assert!(!html.contains(banned), "{banned} leaked into the page");
        }
        assert!(html.contains("h >= 22"));
        assert!(html.contains("h < 7"));
        assert!(html.contains("textContent"));
    }

    #[test]
    fn stack_and_seven_tones_are_in_the_page() {
        let html = document();
        let open_mind = html.find("data-layer=\"open-mind\"").unwrap();
        let command = html.find("data-layer=\"command\"").unwrap();
        assert!(open_mind < command);
        assert_eq!(html.matches("data-tone=\"").count(), TONES.len());
        for tone in TONES {
            let bytes = embedded(html, tone.id);
            let made = sound::wav(tone.id).unwrap();
            assert_eq!(bytes, made, "{}", tone.id);
            assert!(bytes.starts_with(b"RIFF"));
        }
    }

    fn embedded(html: &str, id: &str) -> Vec<u8> {
        let key = format!("data-tone=\"{id}\" src=\"data:audio/wav;base64,");
        let start = html.find(&key).unwrap() + key.len();
        let end = start + html[start..].find('"').unwrap();
        galion_common::base64::decode(&html[start..end]).unwrap()
    }
}
