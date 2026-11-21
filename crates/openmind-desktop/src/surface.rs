// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! First-launch notes, empty states, keys, and motion for the console page.
//!
//! The page document stays the source of the stack, the prompt, and the
//! tones. This module only adds a style and a script around that document,
//! plus three short notes. Arrow keys move focus along the header, Look,
//! the prompt, and the microphone opt-in. No font is fetched. The page already names
//! IBM Plex Sans and IBM Plex Mono (`brand/BRAND.md`) and falls back to the
//! system faces. The content-security policy is `default-src 'none'`, so
//! this layer does not name a second family and does not request a file.

const FILES_EMPTY: &str =
    "<p class=\"notes\" id=\"files-empty\">No files attached. The plus opens the picker.</p>";

const KEPT_EMPTY: &str = "<p class=\"notes\" id=\"kept-empty\">Nothing kept yet. A line you type stays on this page.</p>";

const WELCOME: &str = "<p class=\"notes\" id=\"welcome\">This console stays on this machine. Type look, or press L, to check the stack. Press ? for the keys.</p>";

const KEYS: &str = "<div id=\"keys\" hidden role=\"region\" aria-label=\"Keyboard shortcuts\"><p class=\"kicker\">Keys</p><p><kbd>/</kbd> command <kbd>Enter</kbd> keep <kbd>L</kbd> look <kbd>F</kbd> files <kbd>V</kbd> voice <kbd>M</kbd> mute <kbd>S</kbd> silence <kbd>?</kbd> keys <kbd>Esc</kbd> clear <kbd>Arrows</kbd> move</p></div>";

const SURFACE_CSS: &str = r#"
#keys {
  margin: 12px 0 0;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: var(--panel);
  color: var(--muted);
  font: 13px/1.45 "IBM Plex Mono", ui-monospace, monospace;
}
#keys p { margin: 0; }
#keys .kicker { margin: 0 0 8px; }
#keys kbd { font: inherit; color: var(--fg); }
#look-why { overflow-wrap: break-word; }
#welcome[hidden], #files-empty[hidden], #kept-empty[hidden], #keys[hidden], #look-why[hidden] {
  display: none;
}
@media (prefers-reduced-motion: no-preference) {
  .chip {
    animation: om-rise var(--motion-step) var(--motion-ease);
  }
  #look, .plus, .voice, #mute, #silence, .entries a, .back, .tile, #win-close {
    transition: border-color var(--motion-step) var(--motion-ease), color var(--motion-step) var(--motion-ease), background-color var(--motion-step) var(--motion-ease);
  }
}
@keyframes om-rise {
  from { opacity: 0; transform: translateY(4px); }
  to { opacity: 1; transform: none; }
}
"#;

// Chip text after the em dash. The server tokens are [`crate::files::FileError::message`].
// `too large` is the page's client check before the upload. `not sent` and `refused`
// are the page script, not [`crate::files::FileError`] codes. Eight MiB is
// [`crate::files::MAX_FILE_BYTES`] divided by 1024 twice.

const SURFACE_JS: &str = r##"
(function () {
  var welcome = document.getElementById("welcome");
  var filesEmpty = document.getElementById("files-empty");
  var keptEmpty = document.getElementById("kept-empty");
  var keys = document.getElementById("keys");
  var command = document.getElementById("command");
  var picked = document.getElementById("picked");
  var kept = document.getElementById("kept");
  var faults = {
    "empty": "The file is empty.",
    "too large": "The file is larger than 8 MiB.",
    "too-large": "The file is larger than 8 MiB.",
    "type": "That file type is not accepted.",
    "magic": "The bytes do not match the file name.",
    "traversal": "That path leaves the chosen folder.",
    "outside": "That path was not chosen.",
    "symlink": "A symlink is not opened.",
    "not-found": "That file is not in the chosen folder.",
    "not-file": "That path is not a file.",
    "no-destination": "There is nowhere to store the file yet.",
    "not-loopback": "The intake is not on loopback.",
    "unreachable": "The intake did not answer.",
    "pair": "The intake asked to pair. This desktop does not send a token.",
    "stack": "The intake refused the file.",
    "bad-name": "That name is not a single file name.",
    "bad-json": "The request was not a path.",
    "io": "The disk read failed.",
    "refused": "The request was refused.",
    "not sent": "The listener did not answer."
  };
  function dismissWelcome() {
    if (welcome) welcome.hidden = true;
  }
  function syncEmpty() {
    if (filesEmpty && picked) {
      var filesFilled = picked.children.length > 0;
      filesEmpty.hidden = filesFilled;
      if (filesFilled) dismissWelcome();
    }
    if (keptEmpty && kept) {
      var keptFilled = kept.children.length > 0;
      keptEmpty.hidden = keptFilled;
      var head = kept.previousElementSibling;
      if (head && head.tagName === "P") head.hidden = !keptFilled;
      if (keptFilled) dismissWelcome();
    }
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
  var files = document.getElementById("files");
  var voice = document.getElementById("voice");
  var mute = document.getElementById("mute");
  if (look) look.addEventListener("click", dismissWelcome);
  if (files) files.addEventListener("click", dismissWelcome);
  if (voice) voice.addEventListener("click", dismissWelcome);
  if (mute) mute.addEventListener("click", dismissWelcome);
  var silence = document.getElementById("silence");
  if (silence) silence.addEventListener("click", dismissWelcome);
  var tiles = document.querySelectorAll(".hub .tile");
  var tileAt;
  for (tileAt = 0; tileAt < tiles.length; tileAt = tileAt + 1) {
    tiles[tileAt].addEventListener("click", dismissWelcome);
  }
  if (command) command.addEventListener("input", dismissWelcome);
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
  function focusId(id) {
    var el = document.getElementById(id);
    if (el) el.focus();
  }
  function press(id) {
    dismissWelcome();
    focusId(id);
    clickId(id);
  }
  function controls() {
    var win = document.getElementById("win");
    if (win && !win.hidden) return document.querySelectorAll("#win-open, #win-close");
    return document.querySelectorAll("nav.entries a, #mute, #silence, .hub .tile, #look, #files, #command, #voice, #mic-opt");
  }
  function moveFocus(dir) {
    var list = controls();
    if (!list.length) return;
    var i;
    var at = -1;
    for (i = 0; i < list.length; i = i + 1) {
      if (list[i] === document.activeElement) at = i;
    }
    var next = at < 0
      ? (dir > 0 ? list[0] : list[list.length - 1])
      : list[(at + dir + list.length) % list.length];
    if (next) next.focus();
  }
  function textField(ev) {
    var t = ev.target;
    if (!t) return false;
    if (t === command) return true;
    var tag = t.tagName;
    if (tag === "TEXTAREA" || t.isContentEditable) return true;
    if (tag !== "INPUT") return false;
    var type = (t.getAttribute("type") || "text").toLowerCase();
    return type === "text" || type === "search" || type === "url" || type === "email" || type === "password";
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
    var win = document.getElementById("win");
    if (win && !win.hidden && ev.key !== "Escape" && ev.key.indexOf("Arrow") !== 0) return;
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
    if (ev.key === "ArrowDown" || ev.key === "ArrowRight" || ev.key === "ArrowUp" || ev.key === "ArrowLeft") {
      if (textField(ev) || ev.ctrlKey || ev.metaKey || ev.altKey) return;
      moveFocus((ev.key === "ArrowDown" || ev.key === "ArrowRight") ? 1 : -1);
      ev.preventDefault();
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
    if (key === "l") { press("look"); ev.preventDefault(); }
    else if (key === "f") { press("files"); ev.preventDefault(); }
    else if (key === "v") { press("voice"); ev.preventDefault(); }
    else if (key === "m") { press("mute"); ev.preventDefault(); }
    else if (key === "s") { press("silence"); ev.preventDefault(); }
  });
})();
"##;

/// The console page plus the surface notes, keys, and motion.
///
/// A string that is not the control console is returned unchanged, so the
/// product entries stay as [`crate::page::entry`] wrote them. A console that
/// already carries `id="surface"` is returned unchanged, so the notes are
/// not inserted twice.
#[must_use]
pub fn present(html: &str) -> String {
    if !html.contains("data-desktop=\"control-console\"") || html.contains("id=\"surface\"") {
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
        assert!(html.contains("ArrowRight"));
        assert!(html.contains("ArrowLeft"));
        assert!(html.contains(
            "nav.entries a, #mute, #silence, .hub .tile, #look, #files, #command, #voice, #mic-opt"
        ));
        assert!(html.contains("#win-open, #win-close"));
        assert!(html.contains("<kbd>S</kbd> silence"));
        assert!(html.contains("<kbd>Arrows</kbd> move"));
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
        assert!(html.contains("IBM Plex Sans"));
        assert!(html.contains("IBM Plex Mono"));
        assert!(!html.contains("font-family: \"Inter\""));
        assert!(!html.contains("Geist Mono"));
        assert!(html.contains("om-rise var(--motion-step) var(--motion-ease)"));
        assert!(html.contains("--motion-step: 160ms"));
        assert!(!html.contains("om-rise 180ms"));
        assert!(!html.contains("160ms ease"));
        assert!(html.contains("head.tagName === \"P\""));
        assert_eq!(present(&html), html);
        assert_eq!(present("<p>entry only</p>"), "<p>entry only</p>");
        assert_eq!(
            present(page::entry("/one").unwrap()),
            page::entry("/one").unwrap()
        );
    }

    #[test]
    fn chip_sentences_match_file_error_message() {
        use crate::files::FileError;

        let html = present(page::document());
        let cases = [
            FileError::Empty,
            FileError::TooLarge,
            FileError::Type,
            FileError::Magic,
            FileError::Traversal,
            FileError::Outside,
            FileError::Symlink,
            FileError::NotFound,
            FileError::NotFile,
            FileError::NoDestination,
            FileError::NotLoopback,
            FileError::Unreachable,
            FileError::Pair,
            FileError::StackStatus,
            FileError::BadName,
            FileError::BadJson,
            FileError::Io(String::new()),
        ];
        for err in cases {
            let needle = format!("\"{}\": \"{}\"", err.code(), err.message());
            assert!(html.contains(&needle), "{needle}");
        }
        let large = FileError::TooLarge.message();
        assert!(html.contains(&format!("\"too large\": \"{large}\"")));
    }
}
