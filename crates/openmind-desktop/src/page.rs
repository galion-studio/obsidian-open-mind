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
/// `connect-src 'self'` allows the one same-origin file upload. Any other
/// origin, frames, and form posts are refused.
pub const CSP: &str = "\
default-src 'none'; \
style-src 'unsafe-inline'; \
script-src 'unsafe-inline'; \
media-src data:; \
img-src data:; \
connect-src 'self'; \
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
  flex-direction: column;
  align-items: flex-start;
  border-bottom: 0;
  border-top: 1px solid var(--line);
}
.entries { display: flex; gap: 8px; flex-wrap: wrap; }
.entries a, .back {
  color: var(--fg);
  text-decoration: none;
  border: 1px solid var(--line);
  border-radius: 11px;
  padding: 10px 14px;
  min-height: 44px;
  display: inline-flex;
  align-items: center;
}
.brand { margin: 0; color: var(--muted); font-size: 13px; }
.licence {
  margin: 16px 0 0;
  white-space: pre-wrap;
  font: 12px/1.45 "IBM Plex Mono", ui-monospace, monospace;
  color: var(--muted);
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
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
}
.chips:empty { display: none; }
.chip {
  max-width: 100%;
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 6px 12px;
  background: var(--panel);
  color: var(--fg);
  font-size: 13px;
}
.opt {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin: 12px 0 0;
  color: var(--muted);
  font-size: 14px;
}
.opt input { width: 20px; height: 20px; margin-top: 2px; flex: 0 0 auto; }
audio { display: none; }
@media (max-width: 720px) {
  .app { grid-template-columns: 1fr; }
  .stack { border-right: 0; border-bottom: 1px solid var(--line); }
  .stage { min-height: 50vh; }
  header { flex-wrap: wrap; }
  .entries { width: 100%; }
}
"#;

/// Browser script. The plus posts each chosen file to `/files` on this
/// origin, after the 8 MiB cap. Names are shown with `textContent`. The
/// script does not use `FileReader`. The microphone opens only after the
/// opt-in box is ticked, and only on loopback or a `file:` page. Samples are
/// overwritten in one buffer and then dropped. The level line is the only
/// meter. Night hours and Mute skip tones.
const SCRIPT: &str = r#"
(function () {
  var files = document.getElementById("files");
  var command = document.getElementById("command");
  var voice = document.getElementById("voice");
  var voiceState = document.getElementById("voice-state");
  var level = document.getElementById("mic-level");
  var opt = document.getElementById("mic-opt");
  var picked = document.getElementById("picked");
  var kept = document.getElementById("kept");
  var mute = document.getElementById("mute");
  var live = null;
  var ctx = null;
  var raf = 0;
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
  function loopbackPage() {
    var host = location.hostname;
    if (location.protocol === "file:") return true;
    return host === "localhost" || host === "127.0.0.1" || host === "::1" || host === "[::1]";
  }
  function stopMic() {
    if (raf) { cancelAnimationFrame(raf); raf = 0; }
    if (live) {
      var tracks = live.getTracks();
      var i;
      for (i = 0; i < tracks.length; i = i + 1) tracks[i].stop();
      live = null;
    }
    if (ctx) { ctx.close(); ctx = null; }
    voice.setAttribute("aria-pressed", "false");
    level.textContent = "";
    voiceState.textContent = "Microphone closed. Nothing was saved.";
  }
  function pump(analyser) {
    var buf = new Float32Array(analyser.fftSize);
    function tick() {
      if (!live) return;
      analyser.getFloatTimeDomainData(buf);
      var sum = 0;
      var i;
      for (i = 0; i < buf.length; i = i + 1) sum = sum + buf[i] * buf[i];
      level.textContent = "Level " + String(Math.round(Math.sqrt(sum / buf.length) * 100));
      raf = requestAnimationFrame(tick);
    }
    tick();
  }
  function startMic() {
    if (!opt.checked) {
      voiceState.textContent = "Tick the box before the microphone opens.";
      play("deny");
      return;
    }
    if (!loopbackPage()) {
      voiceState.textContent = "Microphone stays closed away from loopback.";
      play("deny");
      return;
    }
    var devices = navigator.mediaDevices;
    if (!devices || !devices.getUserMedia) {
      voiceState.textContent = "This browser has no microphone API.";
      play("deny");
      return;
    }
    devices.getUserMedia({ audio: true, video: false }).then(function (stream) {
      live = stream;
      ctx = new AudioContext();
      var source = ctx.createMediaStreamSource(stream);
      var analyser = ctx.createAnalyser();
      analyser.fftSize = 256;
      source.connect(analyser);
      voice.setAttribute("aria-pressed", "true");
      voiceState.textContent = "Microphone open on this machine. Audio is not saved or sent.";
      play("ask");
      pump(analyser);
    }).catch(function () {
      stopMic();
      voiceState.textContent = "Microphone permission was refused. Nothing was saved.";
      play("deny");
    });
  }
  function fault(text) {
    var key = "\"error\":\"";
    var at = text.indexOf(key);
    if (at < 0) return "refused";
    var rest = text.slice(at + key.length);
    var end = rest.indexOf("\"");
    if (end < 0) return "refused";
    return rest.slice(0, end);
  }
  function sendFile(file, done) {
    var row = document.createElement("li");
    row.className = "chip";
    row.textContent = file.name;
    picked.appendChild(row);
    if (!file.size || file.size > FILE_MAX) {
      row.textContent = file.name + " — " + (file.size ? "too large" : "empty");
      play("deny");
      done();
      return;
    }
    var pending;
    try {
      pending = fetch("/files", {
        method: "POST",
        credentials: "omit",
        headers: { "X-File-Name": file.name },
        body: file
      });
    } catch (e) {
      row.textContent = file.name + " — not sent";
      play("deny");
      done();
      return;
    }
    pending.then(function (res) {
      return res.text().then(function (text) {
        if (res.ok) {
          row.textContent = file.name + " — sent";
        } else {
          row.textContent = file.name + " — " + fault(text);
          play("deny");
        }
        done();
      });
    }).catch(function () {
      row.textContent = file.name + " — not sent";
      play("deny");
      done();
    });
  }
  function sendAt(list, i) {
    if (i >= list.length) return;
    sendFile(list[i], function () { sendAt(list, i + 1); });
  }
  files.addEventListener("change", function () {
    picked.textContent = "";
    var list = files.files;
    if (list.length) play("hear");
    sendAt(list, 0);
  });
  voice.addEventListener("click", function () {
    if (voice.getAttribute("aria-pressed") === "true") {
      stopMic();
      play("close");
    } else {
      startMic();
    }
  });
  opt.addEventListener("change", function () {
    if (!opt.checked) stopMic();
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

/// The four product lines, in order.
pub const BRAND_LINES: [&str; 4] = [
    "G.A.L.I.O.N. \u{2014} General Autonomous Local Intelligence Open Network",
    "Developed and distributed by Galion Labs",
    "Perpetual Open MIT License",
    "Copyright \u{00A9} 2026 Galion Labs. All rights reserved.",
];

/// Plain MIT licence text shown on the console and the product entries.
pub const MIT_TEXT: &str = "\
MIT License

Copyright (c) 3000 Galion / galion-studio

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
";

/// Galion One (`/one`) or Galion Research (`/research`).
#[must_use]
pub fn entry(path: &str) -> Option<&'static str> {
    match path {
        "/one" => Some(entry_page(
            "one",
            "Galion One",
            "Voice companion. Open this path from the console. The Open Mind stack stays on the console.",
        )),
        "/research" => Some(entry_page(
            "research",
            "Galion Research",
            "Research entry. Hermes stays the engine. The Open Mind stack view stays on the console.",
        )),
        _ => None,
    }
}

fn entry_page(id: &'static str, title: &'static str, lede: &'static str) -> &'static str {
    // Two pages, built once each.
    match id {
        "one" => {
            static PAGE: OnceLock<String> = OnceLock::new();
            PAGE.get_or_init(|| render_entry(id, title, lede))
        }
        _ => {
            static PAGE: OnceLock<String> = OnceLock::new();
            PAGE.get_or_init(|| render_entry(id, title, lede))
        }
    }
}

fn render_entry(id: &str, title: &str, lede: &str) -> String {
    let mut html = String::new();
    html.push_str(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>",
    );
    html.push_str(&esc(title));
    html.push_str("</title><style>");
    html.push_str(CSS);
    html.push_str("</style></head><body><main class=\"stage\" data-entry=\"");
    html.push_str(id);
    html.push_str("\"><div class=\"presence\"><p class=\"kicker\">galion.app</p><h1>");
    html.push_str(&esc(title));
    html.push_str("</h1><p>");
    html.push_str(&esc(lede));
    html.push_str("</p><p><a class=\"back\" href=\"/\">Open Mind console</a></p>");
    html.push_str(&brand_block());
    html.push_str("</div></main></body></html>\n");
    html
}

fn brand_block() -> String {
    let mut html = String::from("<footer>");
    for line in BRAND_LINES {
        html.push_str("<p class=\"brand\">");
        html.push_str(&esc(line));
        html.push_str("</p>");
    }
    html.push_str("<pre class=\"licence\">");
    html.push_str(&esc(MIT_TEXT));
    html.push_str("</pre></footer>");
    html
}

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
<meta http-equiv=\"Content-Security-Policy\" content=\"",
    );
    html.push_str(CSP);
    html.push_str(
        "\"><meta http-equiv=\"Permissions-Policy\" content=\"microphone=(self)\">\
<style>",
    );
    html.push_str(CSS);
    html.push_str("</style></head><body><div class=\"app\" data-desktop=\"control-console\">");
    html.push_str(
        "<header><div><p class=\"kicker\">Obsidian Open Mind</p>\
<h1>Control console</h1></div>\
<nav class=\"entries\" aria-label=\"Products\">\
<a href=\"/one\">Galion One</a>\
<a href=\"/research\">Galion Research</a></nav>\
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
<p>Ask from here. The command stays on this page. No model is called. Tones are 432 Hz.</p>\
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
<ul class=\"chips\" id=\"picked\" aria-label=\"Attached files\"></ul>\
<label class=\"opt\"><input id=\"mic-opt\" type=\"checkbox\"> Allow the microphone on this machine. Audio is not saved and is not sent.</label>\
<p class=\"notes\" id=\"voice-state\">Microphone closed.</p>\
<p class=\"notes\" id=\"mic-level\"></p>\
<div class=\"notes\"><p>Kept on this page</p><ul id=\"kept\"></ul></div>\
</div></main>",
    );
    html.push_str(&brand_block());
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
    html.push_str(&SCRIPT.replace("FILE_MAX", &crate::files::MAX_FILE_BYTES.to_string()));
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
        assert!(html.contains("href=\"/one\""));
        assert!(html.contains("href=\"/research\""));
        assert!(html.contains("aria-label=\"Stack\""));
        for line in BRAND_LINES {
            assert!(html.contains(line), "{line}");
        }
        assert!(!html.contains("border-radius:50%"));
        assert!(!html.contains("border-radius: 50%"));
        assert!(!html.contains("id=\"talk\""));
        let prompt = html.find("class=\"prompt\"").unwrap();
        let chips = html.find("id=\"picked\"").unwrap();
        let kept = html.find("id=\"kept\"").unwrap();
        assert!(prompt < chips && chips < kept);
        assert!(html[chips.saturating_sub(24)..chips].contains("class=\"chips\""));
        assert!(html.contains("aria-label=\"Attached files\""));
        assert!(html.contains("className = \"chip\""));
        assert!(html.contains("fetch(\"/files\""));
    }

    #[test]
    fn page_stays_on_the_machine() {
        let html = document();
        for banned in [
            "http://",
            "https://",
            "ws://",
            "wss://",
            "FileReader",
            "innerHTML",
            "XMLHttpRequest",
            "WebSocket",
            "MediaRecorder",
            "indexedDB",
            "localStorage",
            "createObjectURL",
            "sendBeacon",
        ] {
            assert!(!html.contains(banned), "{banned} leaked into the page");
        }
        assert_eq!(html.matches("fetch(").count(), 1);
        assert!(html.contains("fetch(\"/files\""));
        assert!(html.contains("credentials: \"omit\""));
        assert!(html.contains("connect-src 'self'"));
        assert!(html.contains("h >= 22"));
        assert!(html.contains("h < 7"));
        assert!(html.contains("textContent"));
    }

    #[test]
    fn microphone_is_opt_in_and_loopback_only() {
        let html = document();
        let opt = html.find("id=\"mic-opt\"").unwrap();
        let tag_end = html[opt..].find('>').unwrap() + opt;
        let tag = &html[opt.saturating_sub(30)..tag_end + 1];
        assert!(tag.contains("type=\"checkbox\""));
        assert!(!tag.contains("checked"));
        let guard = html.find("if (!opt.checked)").unwrap();
        let host = html.find("127.0.0.1").unwrap();
        let open = html.find("getUserMedia").unwrap();
        assert!(guard < open);
        assert!(host < open);
        assert!(html.contains("tracks[i].stop()"));
        assert!(html.contains("Nothing was saved"));
        assert!(html.contains("fftSize = 256"));
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

    #[test]
    fn one_and_research_are_entries_and_the_stack_stays_on_the_console() {
        let one = entry("/one").expect("one");
        let research = entry("/research").expect("research");
        assert!(entry("/os").is_none());
        assert!(one.contains("data-entry=\"one\""));
        assert!(research.contains("data-entry=\"research\""));
        assert!(one.contains("Galion One"));
        assert!(research.contains("Galion Research"));
        assert!(one.contains("href=\"/\""));
        assert!(research.contains("Open Mind stack"));
        assert!(!one.contains("aria-label=\"Stack\""));
        for line in BRAND_LINES {
            assert!(one.contains(line), "{line}");
            assert!(research.contains(line), "{line}");
        }
        assert!(one.contains("Permission is hereby granted, free of charge"));
        assert!(research.contains("Permission is hereby granted, free of charge"));
        assert!(!one.contains("https://"));
        assert!(!research.contains("https://"));
    }

    fn embedded(html: &str, id: &str) -> Vec<u8> {
        let key = format!("data-tone=\"{id}\" src=\"data:audio/wav;base64,");
        let start = html.find(&key).unwrap() + key.len();
        let end = start + html[start..].find('"').unwrap();
        galion_common::base64::decode(&html[start..end]).unwrap()
    }
}
