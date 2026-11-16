// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! First-launch notes, empty states, keys, and motion for the console page.
//!
//! The page document stays the source of the stack, the prompt, and the
//! tones. This module only adds a style and a script around that document,
//! plus three short notes. It does not fetch a font: the content-security
//! policy is `default-src 'none'`. The type stack is the brand fallback
//! from `brand/STYLE-GUIDE.md` (Inter, then the system UI; Geist Mono, then
//! a system mono). A machine without those faces keeps the system fonts.

const FILES_EMPTY: &str =
    "<p class=\"notes\" id=\"files-empty\">No files attached. The plus opens the picker.</p>";

const KEPT_EMPTY: &str = "<p class=\"notes\" id=\"kept-empty\">Nothing kept yet. A line you type stays on this page.</p>";

const WELCOME: &str = "<p class=\"notes\" id=\"welcome\">This console stays on this machine. Type look, or press L, to check the stack. Press ? for the keys.</p>";

const KEYS: &str = "<div id=\"keys\" hidden role=\"region\" aria-label=\"Keyboard shortcuts\"><p class=\"kicker\">Keys</p><p><kbd>/</kbd> command <kbd>Enter</kbd> keep <kbd>L</kbd> look <kbd>F</kbd> files <kbd>V</kbd> voice <kbd>M</kbd> mute <kbd>?</kbd> keys <kbd>Esc</kbd> clear</p></div>";

const SURFACE_CSS: &str = r#"
body {
  font-family: "Inter", "Helvetica Neue", Arial, "Liberation Sans", system-ui, sans-serif;
}
.kicker, .stack h2, .stack .state, .stack .seen, .licence, #keys {
  font-family: "Geist Mono", "JetBrains Mono", ui-monospace, Menlo, Consolas, "DejaVu Sans Mono", monospace;
}
#keys {
  margin: 12px 0 0;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: var(--panel);
  color: var(--muted);
  font-size: 13px;
  line-height: 1.45;
}
#keys p { margin: 0; }
#keys .kicker { margin: 0 0 8px; }
#keys kbd { font: inherit; color: var(--fg); }
#welcome[hidden], #files-empty[hidden], #kept-empty[hidden], #keys[hidden], #look-why[hidden] {
  display: none;
}
@media (prefers-reduced-motion: no-preference) {
  .chip {
    animation: om-rise 180ms ease;
  }
  #look, .plus, .voice, #mute, .entries a, .back {
    transition: border-color 160ms ease, color 160ms ease, background-color 160ms ease;
  }
}
@keyframes om-rise {
  from { opacity: 0; transform: translateY(4px); }
  to { opacity: 1; transform: none; }
}
"#;

const SURFACE_JS: &str = r#"
(function () {
  var welcome = document.getElementById("welcome");
  var filesEmpty = document.getElementById("files-empty");
  var keptEmpty = document.getElementById("kept-empty");
  var keys = document.getElementById("keys");
  var command = document.getElementById("command");
  var picked = document.getElementById("picked");
  var kept = document.getElementById("kept");
  var faults = {
    "empty": "the file is empty",
    "too large": "the file is over 8 MiB",
    "too-large": "the file is over 8 MiB",
    "type": "that type is not accepted",
    "magic": "the bytes do not match the name",
    "traversal": "that path leaves the chosen folder",
    "outside": "that path was not chosen",
    "symlink": "a symlink is not opened",
    "not-found": "that file is not in the chosen folder",
    "not-file": "that path is not a file",
    "no-destination": "there is nowhere to store it yet",
    "not-loopback": "the intake is not on loopback",
    "unreachable": "the intake did not answer",
    "pair": "the intake asked to pair",
    "stack": "the intake refused the file",
    "bad-name": "that name is not one file name",
    "bad-json": "the request was not a path",
    "io": "the disk read failed",
    "refused": "the request was refused",
    "not sent": "the listener did not answer"
  };
  function dismissWelcome() {
    if (welcome) welcome.hidden = true;
  }
  function syncEmpty() {
    if (filesEmpty && picked) filesEmpty.hidden = picked.children.length > 0;
    if (keptEmpty && kept) keptEmpty.hidden = kept.children.length > 0;
    if (!picked) return;
    var i;
    for (i = 0; i < picked.children.length; i = i + 1) {
      var row = picked.children[i];
      var text = row.textContent || "";
      var mark = " — ";
      var cut = text.lastIndexOf(mark);
      if (cut < 0) continue;
      var code = text.slice(cut + mark.length);
      var sentence = faults[code];
      if (sentence && row.getAttribute("data-said") !== code) {
        row.setAttribute("data-said", code);
        row.textContent = text.slice(0, cut) + mark + sentence;
      }
    }
  }
  if (picked) {
    new MutationObserver(syncEmpty).observe(picked, { childList: true, characterData: true, subtree: true });
  }
  if (kept) {
    new MutationObserver(function () {
      syncEmpty();
      if (kept.children.length) dismissWelcome();
    }).observe(kept, { childList: true });
  }
  syncEmpty();
  var look = document.getElementById("look");
  if (look) look.addEventListener("click", dismissWelcome);
  function showWhy(data) {
    var probes = data && data.probes;
    if (!probes) return;
    var lines = [];
    var i;
    for (i = 0; i < probes.length; i = i + 1) {
      var item = probes[i];
      if (!item || item.enabled === false || item.state === "up") continue;
      if (item.message) lines.push(item.message);
      else lines.push(String(item.service || "service") + " is " + String(item.state || "unknown") + ".");
    }
    var state = document.getElementById("look-state");
    if (!state || !state.parentNode) return;
    var extra = document.getElementById("look-why");
    if (!lines.length) {
      if (extra) extra.hidden = true;
      return;
    }
    if (!extra) {
      extra = document.createElement("p");
      extra.id = "look-why";
      extra.className = "notes";
      if (state.nextSibling) state.parentNode.insertBefore(extra, state.nextSibling);
      else state.parentNode.appendChild(extra);
    }
    extra.hidden = false;
    extra.textContent = lines.join(" ");
    dismissWelcome();
  }
  if (window.fetch) {
    var nativeFetch = window.fetch.bind(window);
    window.fetch = function (input, init) {
      var pending = nativeFetch(input, init);
      var url = typeof input === "string" ? input : (input && input.url) || "";
      if (url === "/probes" || url.slice(-7) === "/probes") {
        pending.then(function (res) {
          res.clone().json().then(function (data) {
            setTimeout(function () { showWhy(data); }, 0);
          }).catch(function () {});
        }).catch(function () {});
      }
      return pending;
    };
  }
  function clickId(id) {
    var el = document.getElementById(id);
    if (el) el.click();
  }
  function typing(ev) {
    var t = ev.target;
    if (!t) return false;
    if (t === command) return true;
    var tag = t.tagName;
    return tag === "INPUT" || tag === "TEXTAREA" || t.isContentEditable;
  }
  document.addEventListener("keydown", function (ev) {
    if (ev.repeat) return;
    if (ev.key === "Escape") {
      if (keys && !keys.hidden) {
        keys.hidden = true;
        ev.preventDefault();
        return;
      }
      if (command && ev.target === command && command.value) {
        command.value = "";
        ev.preventDefault();
      }
      return;
    }
    if ((ev.key === "k" || ev.key === "K") && (ev.ctrlKey || ev.metaKey) && !ev.altKey) {
      if (command) {
        command.focus();
        ev.preventDefault();
      }
      return;
    }
    if (typing(ev)) return;
    if (ev.ctrlKey || ev.metaKey || ev.altKey) return;
    if (ev.key === "/") {
      if (command) {
        command.focus();
        ev.preventDefault();
      }
      return;
    }
    if (ev.key === "?") {
      if (keys) {
        keys.hidden = !keys.hidden;
        ev.preventDefault();
      }
      return;
    }
    var key = ev.key.toLowerCase();
    if (key === "l") { dismissWelcome(); clickId("look"); ev.preventDefault(); }
    else if (key === "f") { dismissWelcome(); clickId("files"); ev.preventDefault(); }
    else if (key === "v") { dismissWelcome(); clickId("voice"); ev.preventDefault(); }
    else if (key === "m") { dismissWelcome(); clickId("mute"); ev.preventDefault(); }
  });
})();
"#;

/// The console page plus the surface notes, keys, and motion.
///
/// A string that is not the control console is returned unchanged, so the
/// product entries stay as [`crate::page::entry`] wrote them.
#[must_use]
pub fn present(html: &str) -> String {
    if !html.contains("data-desktop=\"control-console\"") {
        return html.to_string();
    }
    let mut out = html.to_string();
    insert_before(&mut out, "<ul class=\"chips\" id=\"picked\"", FILES_EMPTY);
    insert_before(
        &mut out,
        "<div class=\"notes\"><p>Kept on this page</p>",
        KEPT_EMPTY,
    );
    insert_after(&mut out, "<p class=\"notes\" id=\"here\"></p>", WELCOME);
    insert_after(&mut out, WELCOME, KEYS);
    insert_before(
        &mut out,
        "</body>",
        &format!("<style id=\"surface\">{SURFACE_CSS}</style><script>{SURFACE_JS}</script>"),
    );
    out
}

fn insert_before(html: &mut String, anchor: &str, extra: &str) {
    if extra.is_empty() {
        return;
    }
    if let Some(at) = html.find(anchor) {
        html.insert_str(at, extra);
    }
}

fn insert_after(html: &mut String, anchor: &str, extra: &str) {
    if extra.is_empty() {
        return;
    }
    if let Some(at) = html.find(anchor) {
        html.insert_str(at + anchor.len(), extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page;

    #[test]
    fn console_gains_notes_keys_and_motion() {
        let raw = page::document();
        assert!(!raw.contains("id=\"welcome\""));
        assert!(!raw.contains("id=\"surface\""));
        let html = present(raw);
        assert!(html.contains("data-desktop=\"control-console\""));
        assert!(html.contains("id=\"welcome\""));
        assert!(html.contains("id=\"files-empty\""));
        assert!(html.contains("id=\"kept-empty\""));
        assert!(html.contains("id=\"keys\""));
        assert!(html.contains("aria-label=\"Keyboard shortcuts\""));
        assert!(html.contains("prefers-reduced-motion: no-preference"));
        assert!(html.contains("keydown"));
        assert!(html.contains("Escape"));
        assert!(html.contains("\"/probes\""));
        for code in [
            "empty",
            "too large",
            "too-large",
            "type",
            "magic",
            "traversal",
            "outside",
            "symlink",
            "not-found",
            "not-file",
            "no-destination",
            "not-loopback",
            "unreachable",
            "pair",
            "stack",
            "bad-name",
            "bad-json",
            "io",
            "refused",
            "not sent",
        ] {
            assert!(html.contains(&format!("\"{code}\"")), "{code}");
        }
        assert!(html.contains("Copyright (c) 3000 Galion / galion-studio"));
        assert!(html.contains("432 Hz"));
        assert_eq!(present("<p>entry only</p>"), "<p>entry only</p>");
        assert_eq!(
            present(page::entry("/one").unwrap()),
            page::entry("/one").unwrap()
        );
    }
}
