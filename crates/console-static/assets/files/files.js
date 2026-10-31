// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Console file explorer: list, preview, edit (CodeMirror 6), upload, and
// safe file operations against /api/files/. No framework, no external URLs.
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var API = "/api/files/";
  var st = { dir: "", hidden: false, writable: false, posters: false, entries: [], sel: null, editor: null, dirty: false };
  var root = $("fx"), list = $("fx-list"), view = $("fx-view"), msgEl = $("fx-msg");

  function el(tag, attrs, kids) {
    var e = document.createElement(tag);
    if (attrs) for (var k in attrs) {
      if (k === "text") e.textContent = attrs[k];
      else if (k === "cls") e.className = attrs[k];
      else if (k.slice(0, 2) === "on") e.addEventListener(k.slice(2), attrs[k]);
      else e.setAttribute(k, attrs[k]);
    }
    (kids || []).forEach(function (c) { if (c != null) e.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return e;
  }
  function q(path) { return "path=" + encodeURIComponent(path); }
  function size(n) {
    if (n == null) return "";
    var u = ["B", "KB", "MB", "GB", "TB"], i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(n < 10 ? 1 : 0) : n) + " " + u[i];
  }
  function say(t, kind) { msgEl.textContent = t || ""; msgEl.className = "fx-msg" + (kind ? " " + kind : ""); }
  function base(p) { var i = p.lastIndexOf("/"); return i < 0 ? p : p.slice(i + 1); }
  function parent(p) { var i = p.lastIndexOf("/"); return i < 0 ? "" : p.slice(0, i); }

  function get(route, query) {
    return fetch(API + route + (query ? "?" + query : ""), { credentials: "same-origin", cache: "no-store" })
      .then(function (r) {
        return r.json().catch(function () { return { ok: false, error: "HTTP " + r.status }; }).then(function (j) {
          if (r.status === 401) throw new Error("This device is not paired. Open /console on a paired device, or pair this one first.");
          if (!r.ok || j.ok === false) throw new Error(j.error || ("HTTP " + r.status));
          return j;
        });
      });
  }
  function post(route, body) {
    return fetch(API + route, {
      method: "POST", credentials: "same-origin", cache: "no-store",
      headers: { "Content-Type": "application/json", "X-Galion-Files": "1" },
      body: JSON.stringify(body)
    }).then(function (r) {
      return r.json().catch(function () { return { ok: false, error: "HTTP " + r.status }; }).then(function (j) {
        if (!r.ok || j.ok === false) { var e = new Error(j.error || ("HTTP " + r.status)); e.status = r.status; throw e; }
        return j;
      });
    });
  }
  var raw = function (p, dl) { return API + "raw?" + q(p) + (dl ? "&dl=1" : ""); };

  // ---- small dialog (prompt / confirm that works on phones) -----------------
  function ask(question, value, okText) {
    var d = $("fx-dlg"), inp = $("fx-dlg-in");
    $("fx-dlg-q").textContent = question;
    $("fx-dlg-ok").textContent = okText || "OK";
    inp.hidden = value == null;
    inp.value = value || "";
    return new Promise(function (res) {
      d.onclose = function () { res(d.returnValue === "ok" ? (value == null ? true : inp.value.trim()) : null); };
      d.returnValue = "";
      d.showModal();
      if (value != null) { inp.focus(); var dot = inp.value.lastIndexOf("."); inp.setSelectionRange(0, dot > 0 ? dot : inp.value.length); }
    });
  }

  // ---- listing ------------------------------------------------------------
  var LABEL = { folder: "DIR", image: "IMG", video: "VID", audio: "AUD", pdf: "PDF", markdown: "MD", code: "CODE", text: "TXT", archive: "ZIP", packed: "PACK", font: "FONT", document: "DOC", binary: "BIN" };
  function icon(e) {
    var i = el("span", { cls: "fx-ic k-" + e.kind, text: e.kind === "archive" ? (e.name.split(".").pop() || "ARC").toUpperCase().slice(0, 4) : LABEL[e.kind] || "?" });
    if (e.kind === "image" && e.size < 8 * 1048576) {
      i.textContent = ""; i.appendChild(el("img", { src: raw(e.path), alt: "", loading: "lazy", decoding: "async" }));
    } else if (e.kind === "video" && st.posters) {
      var im = el("img", { src: API + "thumb?" + q(e.path) + "&w=160", alt: "", loading: "lazy" });
      im.onerror = function () { im.remove(); i.textContent = "VID"; };
      i.textContent = ""; i.appendChild(im);
    }
    return i;
  }
  function crumbs() {
    var c = $("fx-crumbs"); c.textContent = "";
    c.appendChild(el("a", { href: "#", text: "⌂ root", onclick: function (ev) { ev.preventDefault(); open(""); } }));
    var acc = "";
    st.dir.split("/").filter(Boolean).forEach(function (s) {
      acc = acc ? acc + "/" + s : s;
      var p = acc;
      c.appendChild(document.createTextNode(" / "));
      c.appendChild(el("a", { href: "#" + encodeURIComponent(p), text: s, onclick: function (ev) { ev.preventDefault(); open(p); } }));
    });
  }
  function render() {
    var f = $("fx-filter").value.toLowerCase();
    list.textContent = "";
    var shown = st.entries.filter(function (e) { return !f || e.name.toLowerCase().indexOf(f) >= 0; });
    if (!shown.length) list.appendChild(el("div", { cls: "fx-empty", text: st.entries.length ? "No match." : "Empty folder." }));
    shown.forEach(function (e) {
      var row = el("a", { cls: "fx-row" + (e.hidden ? " fx-hid" : "") + (st.sel && st.sel.path === e.path ? " sel" : ""), href: "#" + encodeURIComponent(e.path), "data-path": e.path,
        onclick: function (ev) { ev.preventDefault(); if (e.dir) open(e.path); else select(e); } }, [
        icon(e),
        el("span", { cls: "fx-nm" }, [e.name + (e.dir ? "/" : ""), el("small", { text: (e.broken ? "broken or outside link" : e.mtime_local || "") })]),
        el("span", { cls: "fx-sz", text: e.dir ? "" : size(e.size) })
      ]);
      list.appendChild(row);
    });
  }
  function open(dir, keepSel) {
    if (!leaveOk()) return Promise.resolve();
    return get("list", q(dir) + (st.hidden ? "&hidden=1" : "")).then(function (j) {
      st.dir = j.path; st.entries = j.entries; st.writable = j.writable;
      root.classList.toggle("ro", !j.writable || j.in_trash);
      var m = $("fx-mode"); m.textContent = j.writable ? "read + write" : "read-only"; m.className = "fx-mode" + (j.writable ? " rw" : "");
      crumbs(); render();
      if (!keepSel) { closeView(); }
      say(j.truncated ? "Showing the first " + j.entries.length + " of " + j.total + "." : "");
      if (location.hash.slice(1) !== encodeURIComponent(dir)) history.replaceState(null, "", "#" + encodeURIComponent(dir));
    }).catch(function (e) { say(e.message, "err"); });
  }

  // ---- preview ------------------------------------------------------------
  function leaveOk() {
    if (st.dirty && !window.confirm("Discard unsaved edits?")) return false;
    st.dirty = false; return true;
  }
  function closeView() {
    if (st.editor) { st.editor.destroy(); st.editor = null; }
    st.sel = null; root.classList.remove("viewing");
    view.textContent = ""; view.appendChild(el("div", { cls: "fx-empty", text: "Pick a file to preview it." }));
    render();
  }
  function select(e) {
    if (!leaveOk()) return;
    if (st.editor) { st.editor.destroy(); st.editor = null; }
    st.sel = e; render();
    root.classList.add("viewing");
    view.textContent = "";
    view.appendChild(el("div", { cls: "fx-empty", text: "Loading…" }));
    get("info", q(e.path)).then(function (i) { st.posters = st.posters || !!i.posters; show(i); }).catch(function (err) { view.textContent = ""; view.appendChild(el("div", { cls: "fx-empty", text: err.message })); });
  }
  function act(label, fn, cls) { return el("button", { type: "button", text: label, cls: cls || "", onclick: fn }); }
  function show(i) {
    view.textContent = "";
    var body = el("div", { cls: "fx-body" });
    var meta = [size(i.size), i.mime, i.mtime_local, i.mode ? "mode " + i.mode : null];
    if (i.media && i.media.duration_s) meta.push(Math.round(i.media.duration_s) + " s");
    if (i.media && i.media.video) meta.push(i.media.video.width + "×" + i.media.video.height + " " + (i.media.video.codec || ""));
    var acts = [act("Download", function () { location.href = raw(i.path, true); })];
    var w = i.writable && !i.in_trash;
    if (w) {
      acts.push(act("Rename", function () { rename(i); }), act("Move", function () { moveCopy(i, "move"); }), act("Copy", function () { moveCopy(i, "copy"); }));
      if (i.preview === "archive") acts.push(act("Extract here", function () { extract(i); }, "pri"));
      acts.push(act("Delete", function () { trash(i); }, "bad"));
    } else if (i.in_trash && i.writable) {
      acts.push(act("Restore to…", function () { moveCopy(i, "move"); }, "pri"));
    }
    view.appendChild(el("div", { cls: "fx-vh" }, [
      act("← Back", function () { if (leaveOk()) closeView(); }, "fx-back"),
      el("h2", { text: i.name })
    ]));
    view.appendChild(el("div", { cls: "fx-meta" }, meta.filter(Boolean).map(function (m) { return el("span", { text: m }); })));
    var actBar = el("div", { cls: "fx-acts" }, acts);
    view.appendChild(actBar);
    view.appendChild(body);
    var p = i.preview;
    if (p === "img") {
      body.appendChild(el("img", { src: raw(i.path), alt: i.name, cls: i.size < 4096 ? "fit-px" : "" }));
    } else if (p === "video") {
      var v = el("video", { src: raw(i.path), controls: "", playsinline: "", preload: "metadata" });
      if (i.posters) v.setAttribute("poster", API + "thumb?" + q(i.path) + "&w=960");
      body.appendChild(v);
    } else if (p === "audio") {
      body.appendChild(el("audio", { src: raw(i.path), controls: "", preload: "metadata" }));
    } else if (p === "pdf") {
      body.appendChild(el("iframe", { src: raw(i.path), title: i.name }));
    } else if (p === "markdown") {
      textView(i, body, actBar, true);
    } else if (p === "editor") {
      textView(i, body, actBar, false);
    } else if (p === "archive") {
      archive(i, body);
    } else {
      hex(i, body, 0);
    }
  }
  function textView(i, body, actBar, md) {
    get("text", q(i.path)).then(function (t) {
      var mode = md ? "rendered" : "editor";
      function paint() {
        if (st.editor) { st.editor.destroy(); st.editor = null; }
        body.textContent = "";
        saveBtn.hidden = mode === "rendered";
        if (t.truncated) body.appendChild(el("p", { cls: "fx-meta", text: "Showing the first " + size(t.text.length) + " (read-only)." }));
        if (mode === "rendered") { body.appendChild(markdown(t.text)); return; }
        var box = el("div", { cls: "fx-ed" }); body.appendChild(box);
        if (!window.GalionEditor) { box.appendChild(el("pre", { cls: "fx-hex", text: t.text })); return; }
        st.editor = window.GalionEditor.open(box, t.text, t.lang || i.lang || "", {
          readOnly: !t.editable,
          onSave: function () { save(); },
          onChange: function () { st.dirty = true; saveBtn.disabled = false; }
        });
      }
      function save() {
        if (!st.editor || !t.editable) return;
        saveBtn.disabled = true; say("Saving…");
        post("write", { path: i.path, content: st.editor.text(), version: t.version }).then(function (r) {
          t.version = r.version; t.text = st.editor.text(); st.dirty = false;
          say("Saved. The old version is in " + (r.backup || ".trash/edits") + ".", "ok");
        }).catch(function (e) {
          saveBtn.disabled = false;
          say(e.status === 409 ? "Someone changed this file since you opened it. Copy your edits, reload, and save again." : e.message, "err");
        });
      }
      var saveBtn = act("Save", save, "pri"); saveBtn.disabled = true;
      if (t.editable) actBar.insertBefore(saveBtn, actBar.firstChild);
      if (md) actBar.appendChild(act("Source / preview", function () { if (!leaveOk()) return; mode = mode === "rendered" ? "editor" : "rendered"; paint(); }));
      paint();
    }).catch(function (e) { body.textContent = e.message; });
  }
  function hex(i, body, off) {
    get("hex", q(i.path) + "&offset=" + off + "&len=4096").then(function (h) {
      var pre = body.querySelector(".fx-hex");
      if (!pre) { pre = el("pre", { cls: "fx-hex" }); body.appendChild(el("p", { cls: "fx-meta", text: "No viewer for this type: raw bytes below. Download to open it elsewhere." })); body.appendChild(pre); }
      pre.textContent += h.dump;
      var more = body.querySelector(".fx-more"); if (more) more.remove();
      if (h.more) body.appendChild(act("More bytes", function () { hex(i, body, h.offset + h.len); }, "fx-more"));
    }).catch(function (e) { body.textContent = e.message; });
  }
  function archive(i, body) {
    body.appendChild(el("p", { cls: "fx-meta", text: "Reading the archive…" }));
    get("archive", q(i.path)).then(function (a) {
      body.textContent = "";
      body.appendChild(el("p", { cls: "fx-meta", text: a.format.toUpperCase() + " · " + a.count + " entries · " + size(a.total_size) + " unpacked" + (a.solid ? " · solid" : "") + (a.truncated ? " · list cut" : "") }));
      var tb = el("tbody");
      a.members.forEach(function (m) {
        tb.appendChild(el("tr", { cls: m.safe ? "" : "bad", title: m.safe ? "" : "Unsafe name: skipped on extract" }, [
          el("td", { text: m.name + (m.dir ? "/" : "") + (m.encrypted ? " 🔒" : "") }),
          el("td", { cls: "n", text: m.dir ? "" : size(m.size) })
        ]));
      });
      body.appendChild(el("table", { cls: "fx-tbl" }, [el("thead", null, [el("tr", null, [el("th", { text: "Name" }), el("th", { text: "Size" })])]), tb]));
    }).catch(function (e) { body.textContent = e.message; });
  }

  // ---- markdown (safe: text nodes only, links http(s) or relative) ---------
  function inline(text, parentEl) {
    var re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*|_[^_]+_)|(\[[^\]]+\]\([^)\s]+\))/g, last = 0, m;
    while ((m = re.exec(text))) {
      if (m.index > last) parentEl.appendChild(document.createTextNode(text.slice(last, m.index)));
      var s = m[0];
      if (m[1]) parentEl.appendChild(el("code", { text: s.slice(1, -1) }));
      else if (m[2]) parentEl.appendChild(el("strong", { text: s.slice(2, -2) }));
      else if (m[3]) parentEl.appendChild(el("em", { text: s.slice(1, -1) }));
      else {
        var lt = s.slice(1, s.indexOf("]")), href = s.slice(s.indexOf("](") + 2, -1);
        if (/^(https?:|#|\/|\.?[\w-])/i.test(href) && !/^(javascript|data|vbscript):/i.test(href)) parentEl.appendChild(el("a", { href: href, rel: "noopener noreferrer", target: "_blank", text: lt }));
        else parentEl.appendChild(document.createTextNode(s));
      }
      last = re.lastIndex;
    }
    if (last < text.length) parentEl.appendChild(document.createTextNode(text.slice(last)));
    return parentEl;
  }
  function markdown(src) {
    var out = el("div", { cls: "fx-md" }), lines = src.split(/\r?\n/), i = 0, listEl = null;
    while (i < lines.length) {
      var l = lines[i];
      if (/^```/.test(l)) {
        var code = []; i++;
        while (i < lines.length && !/^```/.test(lines[i])) code.push(lines[i++]);
        i++; listEl = null;
        out.appendChild(el("pre", null, [el("code", { text: code.join("\n") })])); continue;
      }
      var h = /^(#{1,6})\s+(.*)$/.exec(l);
      if (h) { listEl = null; out.appendChild(inline(h[2], el("h" + h[1].length))); i++; continue; }
      var li = /^\s*([-*+]|\d+\.)\s+(.*)$/.exec(l);
      if (li) {
        if (!listEl) { listEl = el(/\d/.test(li[1]) ? "ol" : "ul"); out.appendChild(listEl); }
        listEl.appendChild(inline(li[2], el("li"))); i++; continue;
      }
      listEl = null;
      if (/^>\s?/.test(l)) { out.appendChild(inline(l.replace(/^>\s?/, ""), el("blockquote"))); i++; continue; }
      if (/^\s*([-*_])\s*\1\s*\1[\s\1]*$/.test(l)) { out.appendChild(el("hr")); i++; continue; }
      if (!l.trim()) { i++; continue; }
      var para = [l]; i++;
      while (i < lines.length && lines[i].trim() && !/^(#{1,6}\s|```|\s*([-*+]|\d+\.)\s|>)/.test(lines[i])) para.push(lines[i++]);
      out.appendChild(inline(para.join(" "), el("p")));
    }
    return out;
  }

  // ---- operations ---------------------------------------------------------
  function done(j, text) { say(text, "ok"); return open(st.dir).then(function () { return j; }); }
  function fail(e) { say(e.message, "err"); }
  function rename(i) {
    ask("New name for " + i.name, i.name, "Rename").then(function (n) {
      if (!n || n === i.name) return;
      post("rename", { path: i.path, name: n }).then(function (j) { return done(j, "Renamed to " + base(j.path) + "."); }).catch(fail);
    });
  }
  function moveCopy(i, how) {
    ask((how === "move" ? "Move " : "Copy ") + i.name + " to folder (path from the root, empty = root)", i.in_trash ? "" : parent(i.path), how === "move" ? "Move" : "Copy").then(function (d) {
      if (d == null) return;
      post(how, { path: i.path, dest: d.replace(/^\/+|\/+$/g, "") }).then(function (j) { return done(j, (how === "move" ? "Moved to " : "Copied to ") + j.path + "."); }).catch(fail);
    });
  }
  function trash(i) {
    ask("Move " + i.name + " to the trash? (Nothing is erased: it goes to .trash and can be moved back.)", null, "Move to trash").then(function (y) {
      if (!y) return;
      post("trash", { path: i.path }).then(function (j) { return done(j, "Moved to " + j.path + "."); }).catch(fail);
    });
  }
  function extract(i) {
    say("Extracting " + i.name + "… large archives take a while.");
    post("extract", { path: i.path }).then(function (j) {
      say("Extracted " + j.files + " files (" + size(j.bytes) + ") into " + j.path + (j.skipped && j.skipped.length ? "; skipped " + j.skipped.length + " unsafe or existing entries" : "") + ".", "ok");
      open(j.path);
    }).catch(fail);
  }
  function newItem(dir) {
    ask(dir ? "New folder name" : "New file name (e.g. notes.md)", "", "Create").then(function (n) {
      if (!n) return;
      var p = st.dir ? st.dir + "/" + n : n;
      (dir ? post("mkdir", { path: st.dir, name: n }) : post("write", { path: p, content: "", create: true }))
        .then(function (j) { return done(j, "Created " + n + ".").then(function () { if (!dir) { var e = st.entries.filter(function (x) { return x.path === p; })[0]; if (e) select(e); } }); }).catch(fail);
    });
  }
  function upload(files) {
    var queue = Array.prototype.slice.call(files), n = 0;
    var bar = el("div", { cls: "fx-prog" }, [el("i")]);
    msgEl.after(bar);
    function next() {
      var f = queue.shift();
      if (!f) { bar.remove(); return done(null, "Uploaded " + n + " file(s)."); }
      return new Promise(function (res) {
        var x = new XMLHttpRequest();
        x.open("PUT", API + "upload?dir=" + encodeURIComponent(st.dir) + "&name=" + encodeURIComponent(f.name));
        x.setRequestHeader("X-Galion-Files", "1");
        x.setRequestHeader("Content-Type", "application/octet-stream");
        x.upload.onprogress = function (ev) { if (ev.lengthComputable) bar.firstChild.style.width = (100 * ev.loaded / ev.total).toFixed(1) + "%"; say("Uploading " + f.name + " (" + size(ev.loaded) + " of " + size(ev.total) + ")…"); };
        x.onload = function () {
          var j = {}; try { j = JSON.parse(x.responseText); } catch (e) { /* not JSON */ }
          if (x.status === 200 && j.ok) { n++; res(next()); } else { bar.remove(); say(f.name + ": " + (j.error || "HTTP " + x.status), "err"); res(); }
        };
        x.onerror = function () { bar.remove(); say(f.name + ": upload cut off", "err"); res(); };
        x.send(f);
      });
    }
    next();
  }

  // ---- wiring ---------------------------------------------------------------
  $("fx-tools").addEventListener("click", function (ev) {
    var a = ev.target.closest("button"); if (!a) return;
    var k = a.getAttribute("data-act");
    if (k === "up") open(parent(st.dir));
    else if (k === "refresh") open(st.dir, true);
    else if (k === "newfile") newItem(false);
    else if (k === "newdir") newItem(true);
    else if (k === "upload") $("fx-up").click();
    else if (k === "trashdir") { st.hidden = true; $("fx-hidden").checked = true; open(".trash"); }
  });
  $("fx-up").addEventListener("change", function () { if (this.files.length) upload(this.files); this.value = ""; });
  $("fx-hidden").addEventListener("change", function () { st.hidden = this.checked; open(st.dir, true); });
  $("fx-filter").addEventListener("input", render);
  window.addEventListener("beforeunload", function (ev) { if (st.dirty) { ev.preventDefault(); ev.returnValue = ""; } });
  document.addEventListener("dragover", function (ev) { if (st.writable) ev.preventDefault(); });
  document.addEventListener("drop", function (ev) { if (!st.writable) return; ev.preventDefault(); if (ev.dataTransfer.files.length) upload(ev.dataTransfer.files); });
  document.addEventListener("keydown", function (ev) { if (ev.key === "Escape" && root.classList.contains("viewing") && !$("fx-dlg").open && leaveOk()) closeView(); });

  var start = decodeURIComponent(location.hash.slice(1) || "");
  get("info", q("")).then(function (i) { st.posters = !!i.posters; }).catch(function () {}).then(function () {
    // A file in the hash opens its folder and then the file.
    return get("info", q(start)).then(function (i) {
      if (i.dir) return open(start);
      return open(parent(start)).then(function () { select(i); });
    }).catch(function () { return open(""); });
  });
})();
