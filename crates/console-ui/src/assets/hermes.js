// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// /hermes room script (vanilla, no build step). Served by the Rust console as
// /_ui/hermes.<hash>.js. Talks only to /api/hermes/* on this origin; the
// gateway key stays on the server. galion OS §11: no animation or blinking.
(function () {
  "use strict";
  var root = document.getElementById("hm");
  if (!root) return;
  var $ = function (id) { return document.getElementById(id); };
  var q = $("hm-q"), send = $("hm-send"), stop = $("hm-stop"), log = $("hm-log"),
    note = $("hm-note"), search = $("hm-search"), list = $("hm-history"),
    mode = $("hm-mode"), status = $("hm-status");
  var state = { thread: root.getAttribute("data-thread") || "", run: "", es: null, busy: false, spendLevel: "" };
  var fmt;
  try {
    fmt = new Intl.DateTimeFormat("en-GB", { timeZone: "Europe/Zagreb", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", timeZoneName: "short" });
  } catch (e) { fmt = null; }
  function stamp(secs) {
    if (!secs) return "";
    var d = new Date(secs * 1000);
    if (!fmt) return d.toISOString().slice(5, 16).replace("T", " ");
    var p = {};
    fmt.formatToParts(d).forEach(function (x) { p[x.type] = x.value; });
    return p.month + "-" + p.day + " " + p.hour + ":" + p.minute + " " + (p.timeZoneName || "");
  }
  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }
  function usd(n) { return "$" + (Number(n) || 0).toFixed(2); }
  function setNote(t, bad) { note.textContent = t || ""; note.className = "hm-note" + (bad ? " t-bad" : ""); }
  function api(path, opts) {
    opts = opts || {};
    opts.credentials = "same-origin";
    opts.headers = Object.assign({ "Accept": "application/json" }, opts.headers || {});
    return fetch(path, opts).then(function (r) {
      return r.text().then(function (t) {
        var j = null;
        try { j = t ? JSON.parse(t) : null; } catch (e) { j = null; }
        if (r.status === 401) throw new Error("This device is not paired. Pair it on the console first.");
        if (!r.ok) throw new Error((j && j.error) || ("The console answered " + r.status + "."));
        return j || {};
      });
    }, function () { throw new Error("The console is not reachable. Check the connection."); });
  }
  function provider() {
    var r = root.querySelector("input[name=hm-provider]:checked");
    return r ? r.value : "";
  }
  function canSend() {
    return !state.busy && q.value.trim().length > 0 && provider() !== "" && state.spendLevel !== "blocked";
  }
  function refreshButtons() {
    send.disabled = !canSend();
    stop.hidden = !state.busy;
    log.setAttribute("aria-busy", state.busy ? "true" : "false");
  }

  // Galion One V2 (owner orders t420u/t421u, 2026-10-30 CET) -----------
  // The orb and sky are pure CSS; this only sets data-state on the room,
  // the caption, the chat layout and the mic level (--lvl). Sounds are
  // synthesized with WebAudio (no samples) and are off by default.
  var reduce = !!(window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  var cap = $("g1-cap");
  var CAPS = { idle: "Ready", listen: "Listening", think: "Thinking", speak: "Answering", error: "Something went wrong" };
  var g1 = { state: "idle", errTimer: 0, lvl: 0, target: 0, raf: 0 };
  function setState(s) {
    if (!CAPS[s]) s = "idle";
    clearTimeout(g1.errTimer);
    g1.state = s;
    root.setAttribute("data-state", s);
    if (cap) cap.textContent = CAPS[s];
    if (s === "error") g1.errTimer = setTimeout(function () { if (g1.state === "error") setState(state.busy ? "think" : "idle"); }, 6000);
    if (s !== "listen") { g1.target = 0; g1.lvl = 0; root.style.removeProperty("--lvl"); }
  }
  function chatMode(on) {
    if (on) root.classList.add("g1-chat"); else root.classList.remove("g1-chat");
  }
  function levelLoop() {
    g1.raf = 0;
    if (g1.state !== "listen" || reduce) return;
    g1.lvl += (g1.target - g1.lvl) * 0.25;
    g1.target *= 0.92;
    root.style.setProperty("--lvl", g1.lvl.toFixed(3));
    if (window.requestAnimationFrame) g1.raf = requestAnimationFrame(levelLoop);
  }
  function micLevel(buf) {
    // RMS of one 50 ms PCM16 frame -> 0..1 (speech sits near 0.02-0.2 RMS).
    if (reduce || !buf || !window.Int16Array) return;
    var a = new Int16Array(buf), n = a.length, s = 0, i;
    if (!n) return;
    for (i = 0; i < n; i += 2) s += a[i] * a[i];
    var rms = Math.sqrt(s / Math.ceil(n / 2)) / 32768;
    var v = Math.min(1, rms * 5);
    if (v > g1.target) g1.target = v;
    if (!g1.raf && window.requestAnimationFrame) g1.raf = requestAnimationFrame(levelLoop);
  }
  function reveal(node, base) {
    // Word-by-word reveal of static text (the greeting). Text stays plain without JS.
    if (!node || reduce) return;
    var words = (node.textContent || "").split(/\s+/).filter(Boolean);
    node.textContent = "";
    words.forEach(function (w, i) {
      var s = el("span", "g1-w", w);
      s.style.animationDelay = (base + i * 60) + "ms";
      node.appendChild(s);
      if (i < words.length - 1) node.appendChild(document.createTextNode(" "));
    });
  }
  function greet() {
    var h = new Date().getHours();
    try { h = Number(new Intl.DateTimeFormat("en-GB", { timeZone: "Europe/Zagreb", hour: "2-digit", hour12: false }).format(new Date())) % 24; } catch (e) { /* local hour */ }
    var hi = $("g1-hi1");
    if (hi) hi.textContent = h < 5 ? "Good night." : h < 12 ? "Good morning." : h < 18 ? "Good afternoon." : "Good evening.";
    reveal(hi, 120);
    reveal($("g1-hi2"), 420);
  }
  // Sound: tiny synthesized cues. Off until the owner turns it on here.
  var SND = "galion-one-sound";
  var snd = { on: false, ctx: null };
  try { snd.on = localStorage.getItem(SND) === "on"; } catch (e) { snd.on = false; }
  function tone(f0, f1, dur, type, peak, at) {
    var c = snd.ctx, t = c.currentTime + (at || 0);
    var o = c.createOscillator(), g = c.createGain();
    o.type = type || "sine";
    o.frequency.setValueAtTime(f0, t);
    if (f1 && f1 !== f0) o.frequency.exponentialRampToValueAtTime(f1, t + dur);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(peak || 0.05, t + Math.min(0.012, dur / 4));
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g);
    g.connect(c.destination);
    o.start(t);
    o.stop(t + dur + 0.02);
  }
  function cue(name) {
    if (!snd.on) return;
    var C = window.AudioContext || window.webkitAudioContext;
    if (!C) return;
    try {
      if (!snd.ctx) snd.ctx = new C();
      if (snd.ctx.state === "suspended" && snd.ctx.resume) snd.ctx.resume();
      if (name === "send") tone(620, 980, 0.09, "sine", 0.045);
      else if (name === "micOn") { tone(520, 520, 0.07, "sine", 0.04); tone(780, 780, 0.09, "sine", 0.04, 0.07); }
      else if (name === "micOff") { tone(780, 780, 0.07, "sine", 0.035); tone(520, 520, 0.09, "sine", 0.035, 0.07); }
      else if (name === "reply") tone(440, 470, 0.16, "triangle", 0.03);
      else if (name === "done") { tone(880, 880, 0.42, "sine", 0.035); tone(1320, 1320, 0.32, "sine", 0.018, 0.03); }
      else if (name === "error") tone(240, 160, 0.24, "sine", 0.05);
      else if (name === "tap") tone(1200, 1200, 0.03, "sine", 0.02);
    } catch (e) { /* no audio here */ }
  }
  var sbtn = $("g1-sound");
  function syncSound() {
    if (!sbtn) return;
    sbtn.textContent = snd.on ? "Sound on" : "Sound off";
    sbtn.setAttribute("aria-pressed", snd.on ? "true" : "false");
    sbtn.setAttribute("aria-label", snd.on ? "Interface sounds are on. Tap to turn them off" : "Interface sounds are off. Tap to turn them on");
  }
  if (sbtn && (window.AudioContext || window.webkitAudioContext)) {
    sbtn.hidden = false;
    syncSound();
    sbtn.addEventListener("click", function () {
      snd.on = !snd.on;
      try { localStorage.setItem(SND, snd.on ? "on" : "off"); } catch (e) { /* private mode */ }
      syncSound();
      cue("tap");
    });
  }
  // Icons are built as DOM nodes (no HTML parsing): [tag, attributes] per shape.
  var ICON = {
    send: [["path", { d: "M12 19V5M5.5 11.5 12 5l6.5 6.5" }]],
    stop: [["rect", { x: "7", y: "7", width: "10", height: "10", rx: "2" }]],
    mic: [["rect", { x: "9", y: "3", width: "6", height: "12", rx: "3" }], ["path", { d: "M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" }]]
  };
  var SVG = "http://www.w3.org/2000/svg";
  function iconBtn(b, ic, label) {
    if (!b) return;
    while (b.firstChild) b.removeChild(b.firstChild);
    if (document.createElementNS) {
      var svg = document.createElementNS(SVG, "svg");
      svg.setAttribute("viewBox", "0 0 24 24");
      svg.setAttribute("aria-hidden", "true");
      ICON[ic].forEach(function (s) {
        var n = document.createElementNS(SVG, s[0]);
        for (var k in s[1]) n.setAttribute(k, s[1][k]);
        svg.appendChild(n);
      });
      b.appendChild(svg);
    }
    var t = el("span", "sr-only");
    t.textContent = label;
    b.appendChild(t);
  }
  iconBtn(send, "send", "Send");
  iconBtn(stop, "stop", "Stop");
  stop.setAttribute("aria-label", "Stop the run");
  function grow() {
    q.style.height = "auto";
    q.style.height = Math.min(q.scrollHeight, Math.round(window.innerHeight * 0.3)) + "px";
  }

  // Text: plain text with ``` fenced blocks shown as <pre>. DOM text nodes only, no HTML parsing.
  function renderText(box, text) {
    box.textContent = "";
    var parts = String(text).split("```");
    parts.forEach(function (part, i) {
      if (i % 2 === 1) {
        var nl = part.indexOf("\n");
        box.appendChild(el("pre", "", nl >= 0 ? part.slice(nl + 1) : part));
      } else if (part) {
        box.appendChild(document.createTextNode(part));
      }
    });
  }
  function msg(kind, text, anim) {
    var a = el("article", "hm-msg hm-" + kind + (anim && !reduce ? " g1-new" : ""));
    chatMode(true);
    a.appendChild(el("p", "hm-who", kind === "user" ? "You" : "Hermes"));
    var t = el("div", "hm-text");
    renderText(t, text || "");
    a.appendChild(t);
    log.appendChild(a);
    return t;
  }
  function tool(name) { var d = el("div", "hm-tool"); d.appendChild(el("span", "mono", name)); log.appendChild(d); return d; }
  function boxMsg(cls, text) { var d = el("div", cls, text); log.appendChild(d); return d; }
  function scrollEnd() {
    log.scrollTop = log.scrollHeight;
    var m = document.getElementById("main");
    // Only a conversation follows the newest line; the idle room stays at the top.
    if (m && /(^| )g1-chat( |$)/.test(root.className)) m.scrollTop = m.scrollHeight;
  }
  // Streaming reveal: while the text only grows and has no code fence, each
  // new chunk is appended as its own fading span (no re-render). Anything
  // else falls back to a full render; the final render is plain text.
  function streamText(box, before, after) {
    if (reduce || after.indexOf("```") >= 0 || box.textContent !== before) { renderText(box, after); return; }
    var chunk = after.slice(before.length);
    if (chunk) box.appendChild(el("span", "g1-c", chunk));
  }
  function clearLog() { log.textContent = ""; }

  // History ------------------------------------------------------------
  function filterHistory() {
    var s = (search.value || "").trim().toLowerCase();
    var shown = 0;
    Array.prototype.forEach.call(list.querySelectorAll("li"), function (li) {
      var hit = !s || li.textContent.toLowerCase().indexOf(s) >= 0;
      li.hidden = !hit;
      if (hit) shown++;
    });
    var c = $("hm-hist-count");
    if (c) c.textContent = (s ? shown + " of " + list.children.length : list.children.length) + " threads · Europe/Zagreb";
  }
  function markCurrent() {
    Array.prototype.forEach.call(list.querySelectorAll("a.hm-item"), function (a) {
      if (a.getAttribute("data-id") === state.thread) a.setAttribute("aria-current", "true");
      else a.removeAttribute("aria-current");
    });
  }
  function loadHistory() {
    return api("/api/hermes/history").then(function (j) {
      list.textContent = "";
      (j.items || []).forEach(function (t) {
        if (!/^[A-Za-z0-9._:-]{1,128}$/.test(t.id || "")) return;
        var li = el("li"), a = el("a", "hm-item");
        a.href = "/hermes?id=" + encodeURIComponent(t.id);
        a.setAttribute("data-id", t.id);
        a.appendChild(el("span", "hm-it", t.title || t.id));
        var meta = [stamp(t.at), (t.count || 0) + " msgs"];
        if (t.cost_usd > 0) meta.push(usd(t.cost_usd));
        if (t.console) meta.push("console");
        a.appendChild(el("span", "hm-im", meta.join(" · ")));
        li.appendChild(a);
        list.appendChild(li);
      });
      var none = $("hm-hist-none"); if (none) none.hidden = list.children.length > 0;
      var err = $("hm-hist-err"); if (err) err.hidden = true;
      markCurrent();
      filterHistory();
    }).catch(function () { /* keep the server-rendered list */ });
  }
  function openThread(id, push) {
    if (state.busy) return;
    state.thread = id;
    markCurrent();
    clearLog();
    var p = boxMsg("hm-pending", "Opening the thread.");
    api("/api/hermes/history?id=" + encodeURIComponent(id)).then(function (j) {
      clearLog();
      var meta = el("p", "hm-thread-meta");
      meta.appendChild(el("span", "hm-it", j.title || id));
      meta.appendChild(el("span", "hm-im", [stamp(j.at), j.model || "", (j.cost_usd > 0 ? usd(j.cost_usd) : "")].filter(Boolean).join(" · ")));
      log.appendChild(meta);
      (j.items || []).forEach(function (it) {
        if (it.kind === "tool") tool(it.name || "tool");
        else if (it.kind === "user" || it.kind === "assistant") msg(it.kind, it.text);
      });
      scrollEnd();
      if (push) history.pushState({ id: id }, "", "/hermes?id=" + encodeURIComponent(id));
      setNote("Thread open. Your next question continues it.");
    }).catch(function (e) { p.className = "hm-err"; p.textContent = "This thread did not load: " + e.message; });
  }
  function newThread(push) {
    if (state.busy) return;
    state.thread = "";
    markCurrent();
    clearLog();
    chatMode(false);
    if (g1.state === "error") setState("idle");
    var w = el("div", "hm-welcome");
    w.appendChild(el("p", "hm-it", "New thread. Ask Hermes a research question."));
    log.appendChild(w);
    if (push) history.pushState({ id: "" }, "", "/hermes");
    setNote(readyNote());
    q.focus();
  }

  // Status -------------------------------------------------------------
  function setPill(key, tone, value) {
    var li = status.querySelector('[data-k="' + key + '"]');
    if (!li) return;
    var dot = li.querySelector(".hm-dot"), v = li.querySelector(".hm-pv");
    if (dot) dot.className = "hm-dot " + tone;
    if (v) v.textContent = value;
    return li;
  }
  function readyNote() {
    if (state.spendLevel === "blocked") return "The research cap is reached. Asking is held until the owner raises it.";
    if (state.spendLevel === "warn") return "Spend is past the warning level. Each ask still counts against the cap.";
    return "Enter sends. Shift+Enter adds a line.";
  }
  function applyGuard(g) {
    var lvl = g.level || "unknown";
    state.spendLevel = lvl;
    var cap = g.cap_usd || 20, warn = g.warn_usd || 16;
    if (typeof g.spent_usd === "number") {
      var li = setPill("spend", lvl === "blocked" ? "bad" : lvl === "warn" ? "warn" : "ok",
        usd(g.spent_usd) + " of " + usd(cap) + (lvl === "blocked" ? " · cap reached" : lvl === "warn" ? " · past the " + usd(warn) + " warning" : ""));
      var m = li && li.querySelector("meter");
      if (m) m.value = Math.min(g.spent_usd, cap);
    } else {
      setPill("spend", "bad", "unknown (asking is held)");
      state.spendLevel = "blocked";
    }
    setPill("guard", g.on ? "ok" : "warn", g.on ? "on · River AI" : "off");
  }
  function refreshStatus() {
    api("/api/hermes/health").then(function (h) {
      if (!h.ok) { setPill("gateway", "bad", h.error || "down"); return; }
      var v = (h.implementation || "hermes") + (h.version ? " " + h.version : "") + " · " + Math.round(h.ms || 0) + " ms";
      if (h.key === false) setPill("gateway", "warn", v + " · key missing");
      else if (h.auth_ok === false) setPill("gateway", "warn", v + " · key refused");
      else setPill("gateway", "ok", v);
    }).catch(function (e) { setPill("gateway", "bad", e.message); });
    api("/api/hermes/guard").then(function (g) { applyGuard(g); if (!state.busy) setNote(readyNote()); refreshButtons(); })
      .catch(function () { setPill("guard", "bad", "no answer"); state.spendLevel = "blocked"; refreshButtons(); });
  }

  // Model picker (t364u/t365u) -------------------------------------------
  // The old Node panel (console-ide hermes-panel.tsx) had a searchable model
  // box with stars and a check mark; this is that box for the Rust page. The
  // lists come from /api/hermes/models?provider= (fetched server-side, keys
  // stay there); choices and stars are saved through /api/hermes/prefs with
  // localStorage as the fallback. Tap targets are >= 44px; nothing is hover-only.
  var LS = "galion-hermes-picker-v1";
  var LABELS = { openrouter: "OpenRouter", river: "River AI", laptop: "Local laptop", runpod: "RunPod" };
  var pick = { provider: "", models: { openrouter: "", river: "" }, stars: { openrouter: [], river: [] }, names: {} };
  var cat = {}, tab = "openrouter", sheetPushed = false, ignorePop = false, lastFocus = null;
  var pickBtn = $("hm-pick"), pickV = $("hm-pick-v"), sheet = $("hm-sheet"), msearch = $("hm-msearch"),
    mlist = $("hm-mlist"), mcount = $("hm-mcount"), sheetX = $("hm-sheet-x");
  function isCat(p) { return p === "openrouter" || p === "river"; }
  function idOk(m) { return typeof m === "string" && /^[A-Za-z0-9._:\/@+-]{1,160}$/.test(m); }
  function radio(p) { return root.querySelector('input[name=hm-provider][value="' + p + '"]'); }
  function usable(p) { var r = radio(p); return !!(r && !r.disabled); }
  function merge(j) {
    if (!j || typeof j !== "object") return;
    if (typeof j.provider === "string" && LABELS[j.provider]) pick.provider = j.provider;
    ["openrouter", "river"].forEach(function (p) {
      var m = j.models && j.models[p];
      if (m === "" || idOk(m)) pick.models[p] = m;
      var s = j.stars && j.stars[p];
      if (Array.isArray(s)) pick.stars[p] = s.filter(idOk).slice(0, 48);
    });
    if (j.names && typeof j.names === "object") Object.keys(j.names).forEach(function (k) { if (idOk(k) && typeof j.names[k] === "string") pick.names[k] = j.names[k].slice(0, 120); });
  }
  function saveLocal() { try { localStorage.setItem(LS, JSON.stringify(pick)); } catch (e) { /* private mode */ } }
  function savePrefs(body) {
    saveLocal();
    api("/api/hermes/prefs", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })
      .catch(function () { /* kept in this browser */ });
  }
  function modelName(p, id) {
    if (!id) return p === "river" ? "River AI default" : "Gateway default (paid research model)";
    var c = cat[p], i;
    if (c) for (i = 0; i < c.models.length; i++) if (c.models[i].id === id) return c.models[i].name || id;
    return pick.names[id] || id;
  }
  function choiceText() {
    var p = provider() || pick.provider || "openrouter";
    if (!isCat(p)) return LABELS[p] || p;
    return (LABELS[p] || p) + " · " + modelName(p, pick.models[p] || "");
  }
  function syncModel() {
    var t = choiceText();
    if (pickV) pickV.textContent = t;
    setPill("model", isCat(provider()) ? "ok" : "warn", t);
    if (pickBtn) pickBtn.disabled = !(usable("openrouter") || usable("river"));
  }
  function setProvider(p, save) {
    var r = radio(p);
    if (!r || r.disabled) return false;
    r.checked = true;
    pick.provider = p;
    syncModel();
    refreshButtons();
    if (save) savePrefs({ action: "select", provider: p, model: isCat(p) ? (pick.models[p] || "") : "" });
    return true;
  }
  function loadCat(p) {
    if (cat[p]) return Promise.resolve(cat[p]);
    return api("/api/hermes/models?provider=" + encodeURIComponent(p)).then(function (j) {
      var rows = (j.models || []).filter(function (m) { return m && idOk(m.id); });
      cat[p] = { models: rows, def: (j["default"] && j["default"].name) || modelName(p, ""), stale: !!j.stale };
      return cat[p];
    });
  }
  function meta(m) {
    var bits = [m.id];
    if (m.alias) bits.push("gateway alias");
    if (m.ctx) bits.push(m.ctx >= 1000 ? Math.round(m.ctx / 1000) + "k ctx" : m.ctx + " ctx");
    if (typeof m["in"] === "number" && typeof m.out === "number") bits.push("$" + m["in"].toFixed(2) + " / $" + m.out.toFixed(2) + " per M tok");
    return bits.join(" · ");
  }
  function row(p, m, star) {
    var sel = (pick.models[p] || "") === m.id && provider() === p;
    var li = el("li", "hm-mrow");
    if (star) {
      var on = pick.stars[p].indexOf(m.id) >= 0;
      var s = el("button", "hm-star", on ? "\u2605" : "\u2606");
      s.type = "button";
      s.setAttribute("data-star", m.id);
      s.setAttribute("aria-pressed", on ? "true" : "false");
      s.setAttribute("aria-label", (on ? "Remove " : "Add ") + (m.name || m.id) + (on ? " from favourites" : " to favourites"));
      li.appendChild(s);
    } else li.appendChild(el("span", "hm-star-gap"));
    var b = el("button", "hm-mpick");
    b.type = "button";
    b.setAttribute("data-pick", m.id);
    b.setAttribute("aria-pressed", sel ? "true" : "false");
    var tx = el("span", "hm-mtx");
    tx.appendChild(el("span", "hm-mn", m.name || m.id));
    tx.appendChild(el("span", "hm-mi", m.id ? meta(m) : "Uses the gateway's own choice for this provider"));
    b.appendChild(tx);
    var ck = el("span", "hm-ck", sel ? "\u2713" : "");
    ck.setAttribute("aria-hidden", "true");
    b.appendChild(ck);
    li.appendChild(b);
    return li;
  }
  function renderList() {
    var c = cat[tab];
    mlist.textContent = "";
    if (!c) return;
    var words = (msearch.value || "").toLowerCase().split(/\s+/).filter(Boolean);
    function hit(m) { var h = ((m.name || "") + " " + m.id).toLowerCase(); return words.every(function (w) { return h.indexOf(w) >= 0; }); }
    var byId = {};
    c.models.forEach(function (m) { byId[m.id] = m; });
    var stars = pick.stars[tab].map(function (id) { return byId[id] || { id: id, name: pick.names[id] || id }; }).filter(hit);
    var rest = c.models.filter(function (m) { return pick.stars[tab].indexOf(m.id) < 0 && hit(m); });
    var frag = document.createDocumentFragment();
    var def = { id: "", name: c.def };
    if (!words.length || hit(def)) frag.appendChild(row(tab, def, false));
    if (stars.length) { frag.appendChild(el("li", "hm-mhead", "Favourites")); stars.forEach(function (m) { frag.appendChild(row(tab, m, true)); }); }
    if (rest.length) { frag.appendChild(el("li", "hm-mhead", stars.length ? "All models" : "Models")); rest.forEach(function (m) { frag.appendChild(row(tab, m, true)); }); }
    mlist.appendChild(frag);
    var n = stars.length + rest.length;
    mcount.textContent = (words.length ? n + " of " + c.models.length : c.models.length) + " models" + (c.stale ? " · list may be out of date" : "") + ". Star a model to keep it at the top.";
  }
  function showTab(p) {
    tab = p;
    Array.prototype.forEach.call(sheet.querySelectorAll(".hm-tab"), function (b) {
      var bp = b.getAttribute("data-p");
      b.setAttribute("aria-pressed", bp === p ? "true" : "false");
      b.disabled = !usable(bp);
    });
    mlist.textContent = "";
    if (!usable(p)) { mcount.textContent = (LABELS[p] || p) + " is not available right now."; return; }
    if (!cat[p]) mcount.textContent = "Loading the " + (LABELS[p] || p) + " model list.";
    loadCat(p).then(function () { if (tab === p) renderList(); })
      .catch(function (e) { if (tab === p) mcount.textContent = "The model list did not load: " + e.message; });
  }
  function openSheet() {
    if (!sheet || !sheet.hidden) return;
    lastFocus = document.activeElement;
    var p = provider();
    sheet.hidden = false;
    document.documentElement.classList.add("hm-lock");
    pickBtn.setAttribute("aria-expanded", "true");
    msearch.value = "";
    showTab(isCat(p) && usable(p) ? p : (usable("openrouter") ? "openrouter" : "river"));
    try { history.pushState(Object.assign({}, history.state || {}, { sheet: 1 }), ""); sheetPushed = true; } catch (e) { sheetPushed = false; }
    // Focus the list's close button first: on a phone, focusing the search
    // box would open the keyboard over half the list.
    (sheetX || msearch).focus();
  }
  function closeSheet(fromPop) {
    if (!sheet || sheet.hidden) return;
    sheet.hidden = true;
    document.documentElement.classList.remove("hm-lock");
    pickBtn.setAttribute("aria-expanded", "false");
    if (sheetPushed && !fromPop) { ignorePop = true; history.back(); }
    sheetPushed = false;
    if (lastFocus && lastFocus.focus) lastFocus.focus(); else pickBtn.focus();
  }
  function choose(p, id) {
    pick.models[p] = id;
    var c = cat[p];
    if (id && c) c.models.forEach(function (m) { if (m.id === id && m.name) pick.names[id] = m.name; });
    setProvider(p, false);
    savePrefs({ action: "select", provider: p, model: id });
    syncModel();
    closeSheet(false);
    setNote("Next question goes to " + choiceText() + ".");
  }
  function toggleStar(id) {
    var s = pick.stars[tab], i = s.indexOf(id), on = i < 0;
    if (on) { s.unshift(id); if (s.length > 48) s.length = 48; } else s.splice(i, 1);
    var c = cat[tab];
    if (c) c.models.forEach(function (m) { if (m.id === id && m.name) pick.names[id] = m.name; });
    savePrefs({ action: "star", provider: tab, model: id, on: on });
    var top = mlist.scrollTop;
    renderList();
    mlist.scrollTop = top;
    var b = mlist.querySelector('[data-star="' + (window.CSS && CSS.escape ? CSS.escape(id) : id) + '"]');
    if (b) b.focus();
  }
  function selectedModel() {
    var p = provider();
    return isCat(p) ? (pick.models[p] || "") : "";
  }
  if (pickBtn && sheet) {
    pickBtn.addEventListener("click", openSheet);
    sheetX.addEventListener("click", function () { closeSheet(false); });
    sheet.addEventListener("click", function (e) {
      if (e.target === sheet) { closeSheet(false); return; }
      var t = e.target.closest && e.target.closest("button");
      if (!t) return;
      if (t.classList.contains("hm-tab")) { if (!t.disabled) { msearch.value = ""; showTab(t.getAttribute("data-p")); } }
      else if (t.hasAttribute("data-star")) toggleStar(t.getAttribute("data-star"));
      else if (t.hasAttribute("data-pick")) choose(tab, t.getAttribute("data-pick"));
    });
    msearch.addEventListener("input", renderList);
    msearch.addEventListener("keydown", function (e) {
      if (e.key === "Enter") { var b = mlist.querySelector("[data-pick]:not([data-pick=''])") || mlist.querySelector("[data-pick]"); if (b) { e.preventDefault(); b.click(); } }
    });
    sheet.addEventListener("keydown", function (e) {
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeSheet(false); return; }
      if (e.key !== "Tab") return;
      var f = Array.prototype.filter.call(sheet.querySelectorAll("button,input"), function (n) { return !n.disabled && n.offsetParent !== null; });
      if (!f.length) return;
      if (e.shiftKey && document.activeElement === f[0]) { e.preventDefault(); f[f.length - 1].focus(); }
      else if (!e.shiftKey && document.activeElement === f[f.length - 1]) { e.preventDefault(); f[0].focus(); }
    });
  }
  function initPicker() {
    try { merge(JSON.parse(localStorage.getItem(LS) || "null")); } catch (e) { /* none */ }
    if (pick.provider) setProvider(pick.provider, false);
    syncModel();
    api("/api/hermes/prefs").then(function (j) {
      if (j && j.stored) { var names = pick.names; merge(j); pick.names = names; saveLocal(); if (pick.provider) setProvider(pick.provider, false); }
      syncModel();
    }).catch(function () { /* local choices stay */ });
  }

  // Asking -------------------------------------------------------------
  // After a run, tag the answer with the model the gateway actually used
  // (the session's model from history), not just the one that was picked.
  function tagAnswer(out) {
    var id = state.thread;
    if (!id || !out || !out.parentNode) return;
    api("/api/hermes/history?id=" + encodeURIComponent(id)).then(function (j) {
      if (!j || !j.model || !out.parentNode) return;
      var t = out.parentNode.querySelector(".hm-answer-meta") || el("p", "hm-answer-meta");
      t.textContent = "Answered by " + j.model;
      out.parentNode.appendChild(t);
    }).catch(function () { /* meta only */ });
  }
  function finish(text) {
    state.busy = false;
    state.run = "";
    if (state.es) { try { state.es.close(); } catch (e) { /* closed */ } state.es = null; }
    refreshButtons();
    setNote(text || readyNote(), false);
    var bad = !text || text === "The run failed." || text === "Not sent.";
    var mic = window.HermesUI && window.HermesUI.mic && window.HermesUI.mic.state();
    setState(bad ? "error" : (mic === "live" || mic === "connecting") ? "listen" : "idle");
    cue(bad ? "error" : text === "Done." ? "done" : "tap");
    loadHistory();
    refreshStatus();
  }
  function followRun(runId, out, pending) {
    var got = "", done = false;
    function ended(final) {
      if (done) return;
      done = true;
      if (pending && pending.parentNode) pending.parentNode.removeChild(pending);
      final();
    }
    function poll() {
      api("/api/hermes/runs/" + encodeURIComponent(runId)).then(function (r) {
        if (r.status === "completed") ended(function () { if (!got && r.output) renderText(out, r.output); finish("Done."); tagAnswer(out); });
        else if (r.status === "failed") ended(function () { boxMsg("hm-err", "The run failed: " + (r.error || "no reason given")); finish("The run failed."); });
        else if (r.status === "cancelled") ended(function () { boxMsg("hm-pending", "Stopped."); finish("Stopped."); });
        else setTimeout(poll, 2500);
      }).catch(function (e) { ended(function () { boxMsg("hm-err", e.message); finish(); }); });
    }
    if (!window.EventSource) { poll(); return; }
    var es = new EventSource("/api/hermes/runs/" + encodeURIComponent(runId) + "/events");
    state.es = es;
    es.onmessage = function (ev) {
      var e; try { e = JSON.parse(ev.data); } catch (x) { return; }
      switch (e.event) {
        case "run.started":
          if (e.session_id && /^[A-Za-z0-9._:-]{1,128}$/.test(e.session_id)) {
            state.thread = e.session_id;
            history.replaceState({ id: e.session_id }, "", "/hermes?id=" + encodeURIComponent(e.session_id));
          }
          break;
        case "message.delta":
          if (pending && pending.parentNode) { pending.textContent = "Writing."; }
          if (!got) { setState("speak"); cue("reply"); }
          streamText(out, got, got + (e.delta || ""));
          got += e.delta || ""; scrollEnd(); break;
        case "tool.started":
          if (pending) pending.textContent = "Using " + (e.tool || "a tool") + ".";
          log.insertBefore(el("div", "hm-tool", "tool · " + (e.tool || "tool")), out.parentNode);
          break;
        case "status":
          if (pending && e.text) pending.textContent = String(e.text).slice(0, 160);
          break;
        case "run.completed":
          ended(function () { if (!got && e.output) renderText(out, e.output); scrollEnd(); finish("Done."); tagAnswer(out); });
          break;
        case "run.failed":
          ended(function () { boxMsg("hm-err", "The run failed: " + (e.error || "no reason given")); finish("The run failed."); });
          break;
        case "run.cancelled":
          ended(function () { boxMsg("hm-pending", "Stopped."); finish("Stopped."); });
          break;
      }
    };
    es.onerror = function () {
      if (done) return;
      try { es.close(); } catch (x) { /* closed */ }
      state.es = null;
      poll();
    };
  }
  function ask() {
    if (!canSend()) return;
    var text = q.value.trim();
    var body = { input: text, provider: provider(), mode: mode ? mode.value : "deep" };
    var chosen = selectedModel();
    if (chosen) body.model = chosen;
    if (state.thread) body.session_id = state.thread;
    var w = $("hm-welcome"); if (w && w.parentNode) w.parentNode.removeChild(w);
    Array.prototype.forEach.call(log.querySelectorAll(".hm-welcome"), function (n) { n.parentNode.removeChild(n); });
    msg("user", text, true);
    var out = msg("assistant", "", true);
    setState("think");
    cue("send");
    var pending = el("p", "hm-pending", "Asking Hermes. Research can take a few minutes.");
    out.parentNode.appendChild(pending);
    scrollEnd();
    q.value = "";
    grow();
    state.busy = true;
    refreshButtons();
    setNote("Hermes is working.");
    api("/api/hermes/ask", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })
      .then(function (j) {
        if (j.spend) applyGuard({ level: j.spend.level, spent_usd: j.spend.spent_usd, cap_usd: j.spend.cap_usd, warn_usd: j.spend.warn_usd, on: true });
        if (j.warn) boxMsg("hm-warnbox", "Heads up: research spend is past " + usd(j.spend && j.spend.warn_usd || 16) + " of the " + usd(j.spend && j.spend.cap_usd || 20) + " cap.");
        if (!j.run_id) throw new Error("The gateway did not return a run.");
        state.run = j.run_id;
        followRun(j.run_id, out, pending);
      })
      .catch(function (e) {
        if (pending.parentNode) pending.parentNode.removeChild(pending);
        if (!out.textContent) out.parentNode.parentNode.removeChild(out.parentNode);
        boxMsg("hm-err", e.message);
        q.value = text;
        finish("Not sent.");
        setNote(e.message, true);
      });
  }
  function stopRun() {
    if (!state.run) return;
    stop.disabled = true;
    api("/api/hermes/runs/" + encodeURIComponent(state.run) + "/stop", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" })
      .then(function () { setNote("Stopping."); })
      .catch(function (e) { setNote(e.message, true); })
      .then(function () { stop.disabled = false; });
  }

  // Wiring ------------------------------------------------------------
  q.disabled = false;
  q.addEventListener("keydown", function (e) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); ask(); }
  });
  q.addEventListener("input", function () { grow(); refreshButtons(); if (g1.state === "error") setState("idle"); });
  send.addEventListener("click", ask);
  stop.addEventListener("click", stopRun);
  root.addEventListener("change", function (e) {
    if (e.target && e.target.name === "hm-provider") {
      pick.provider = e.target.value;
      syncModel();
      refreshButtons();
      savePrefs({ action: "select", provider: pick.provider, model: isCat(pick.provider) ? (pick.models[pick.provider] || "") : "" });
    }
  });
  search.addEventListener("input", filterHistory);
  search.addEventListener("keydown", function (e) {
    if (e.key === "Enter") { var a = list.querySelector("li:not([hidden]) a"); if (a) { e.preventDefault(); a.click(); } }
  });
  list.addEventListener("click", function (e) {
    var a = e.target.closest && e.target.closest("a.hm-item");
    if (!a || e.metaKey || e.ctrlKey || e.shiftKey) return;
    e.preventDefault();
    openThread(a.getAttribute("data-id"), true);
  });
  var nb = $("hm-new");
  if (nb) nb.addEventListener("click", function (e) { e.preventDefault(); newThread(true); });
  window.addEventListener("popstate", function (e) {
    if (ignorePop) { ignorePop = false; return; }
    if (sheet && !sheet.hidden) { closeSheet(true); return; }
    if (e.state && e.state.sheet) { history.back(); return; }
    var id = (e.state && e.state.id) || new URLSearchParams(location.search).get("id") || "";
    if (id) openThread(id, false); else newThread(false);
  });
  document.addEventListener("keydown", function (e) {
    var ae = document.activeElement, typing = ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.tagName === "SELECT");
    if (e.key === "/" && !typing && (!sheet || sheet.hidden)) { e.preventDefault(); search.focus(); }
    if (e.key === "Escape" && state.busy) stopRun();
  });
  initPicker();
  greet();
  if (log.querySelector(".hm-msg")) chatMode(true);
  var hist = $("hm-hist");
  if (hist && window.matchMedia && window.matchMedia("(min-width: 900px)").matches) hist.open = true;
  refreshStatus();
  setInterval(function () { if (!document.hidden && !state.busy) refreshStatus(); }, 30000);
  scrollEnd();
  setNote(readyNote());
  refreshButtons();

  // Mic hook (live dictation below uses it). A mic
  // module may call HermesUI.mount(node) to place its button, then
  // HermesUI.insert(text) with partial/final transcripts and HermesUI.send().
  window.HermesUI = {
    mount: function (node) { var s = $("hm-mic-slot"); if (s && node) { s.appendChild(node); s.hidden = false; } },
    insert: function (text) { q.value = (q.value ? q.value + " " : "") + String(text || ""); refreshButtons(); },
    send: ask,
    busy: function () { return state.busy; }
  };

  // Live dictation (owner order t362u): tap the mic, speech streams to
  // galion-stt through /api/stt/stream, partial text shows provisionally
  // under the text box and each final line lands in the text box, where it
  // can be edited before sending. Tap again to stop. States are plain text
  // labels (no flashing, section 11).
  (function mic() {
    var slot = $("hm-mic-slot");
    var partial = $("hm-partial");
    if (!slot) return;
    var worklet = slot.getAttribute("data-worklet") || "";
    var healthUrl = slot.getAttribute("data-stt") || "/api/stt/health";
    var streamUrl = slot.getAttribute("data-stream") || "/api/stt/stream";
    var btn = el("button", "hm-btn hm-mic");
    btn.type = "button";
    btn.id = "hm-mic";
    btn.setAttribute("aria-pressed", "false");
    window.HermesUI.mount(btn);
    var LABEL = {
      idle: ["Mic", "Start dictation"],
      checking: ["Mic …", "Checking the speech service"],
      connecting: ["Connecting…", "Connecting to the speech service"],
      live: ["Stop mic", "Listening. Tap to stop dictation"],
      finishing: ["Finishing…", "Waiting for the last words"],
      blocked: ["Mic blocked", "Microphone access is blocked. Allow it in the browser's site settings, then tap to retry"],
      offline: ["STT offline", "The speech service is offline. Tap to check again"],
      unsupported: ["No mic", "This browser cannot stream audio here"]
    };
    var m = { state: "idle", ws: null, ctx: null, media: null, node: null, opened: false, stopping: false, timer: 0, parts: {} };
    function show(s, detail) {
      m.state = s;
      var l = LABEL[s];
      var was = g1.state === "listen";
      iconBtn(btn, "mic", l[0]);
      btn.title = detail || l[1];
      if (s === "live" || s === "connecting") { setState("listen"); if (s === "live") cue("micOn"); }
      else if (was) { setState(state.busy ? "think" : "idle"); cue("micOff"); }
      btn.setAttribute("aria-label", detail || l[1]);
      btn.setAttribute("aria-pressed", s === "live" || s === "connecting" || s === "finishing" ? "true" : "false");
      btn.setAttribute("data-state", s);
      btn.disabled = s === "unsupported" || s === "checking" || s === "finishing";
    }
    function setPartial() {
      if (!partial) return;
      var keys = Object.keys(m.parts).sort(function (a, b) { return a - b; });
      var t = keys.map(function (k) { return m.parts[k]; }).join(" ").trim();
      partial.textContent = t;
      partial.hidden = !t;
    }
    function commit(text) {
      text = String(text || "").trim();
      if (!text) return;
      var cur = q.value;
      q.value = cur + (cur && !/\s$/.test(cur) ? " " : "") + text;
      refreshButtons();
    }
    function health() {
      return api(healthUrl).then(function (j) { return !!(j && j.ok); }, function () { return false; });
    }
    function releaseAudio() {
      if (m.node) { try { m.node.port.postMessage("stop"); m.node.disconnect(); } catch (e) { /* gone */ } }
      if (m.media) m.media.getTracks().forEach(function (t) { t.stop(); });
      if (m.ctx) { try { m.ctx.close(); } catch (e) { /* closed */ } }
      m.node = null; m.media = null; m.ctx = null;
    }
    function cleanup(next, detail) {
      clearTimeout(m.timer);
      releaseAudio();
      var ws = m.ws;
      m.ws = null;
      if (ws) { ws.onclose = null; ws.onerror = null; ws.onmessage = null; try { ws.close(); } catch (e) { /* closed */ } }
      // Any partial left over is kept as text: better than losing words.
      Object.keys(m.parts).forEach(function (k) { commit(m.parts[k]); });
      m.parts = {};
      setPartial();
      m.stopping = false;
      show(next, detail);
    }
    function onMessage(e) {
      if (typeof e.data !== "string") return;
      var j = null;
      try { j = JSON.parse(e.data); } catch (x) { return; }
      if (!j) return;
      var line = j.line != null ? String(j.line) : "0";
      if (j.type === "partial") { m.parts[line] = String(j.text || ""); setPartial(); }
      else if (j.type === "final") { delete m.parts[line]; setPartial(); commit(j.text); }
      else if (j.type === "error") { setNote("Dictation: " + (j.text || j.error || "the speech service reported an error."), true); }
    }
    function openSocket() {
      var url = (location.protocol === "https:" ? "wss://" : "ws://") + location.host + streamUrl;
      var ws;
      try { ws = new WebSocket(url); } catch (e) { cleanup("offline"); return; }
      ws.binaryType = "arraybuffer";
      m.ws = ws;
      m.opened = false;
      ws.onopen = function () {
        if (m.ws !== ws) return;
        m.opened = true;
        if (m.stopping) { ws.send(JSON.stringify({ type: "stop" })); show("finishing"); return; }
        show("live");
        setNote("Listening. Speak, then tap Stop mic. You can edit the text before sending.");
      };
      ws.onmessage = onMessage;
      ws.onerror = function () { /* onclose follows */ };
      ws.onclose = function () {
        if (m.ws !== ws) return;
        if (!m.opened) { cleanup("offline"); setNote("The speech service is offline (or this device is not paired). Typing still works.", true); return; }
        cleanup("idle");
        if (!m.stopping) setNote(readyNote());
      };
    }
    function start() {
      if (!(navigator.mediaDevices && navigator.mediaDevices.getUserMedia && window.AudioWorkletNode && window.WebSocket)) { show("unsupported"); return; }
      show("connecting");
      health().then(function (ok) {
        if (m.state !== "connecting") return null;
        if (!ok) { show("offline"); setNote("The speech service is offline. Typing still works.", true); return null; }
        return navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 } }).then(function (media) {
          if (m.state !== "connecting") { media.getTracks().forEach(function (t) { t.stop(); }); return null; }
          m.media = media;
          var Ctx = window.AudioContext || window.webkitAudioContext;
          m.ctx = new Ctx();
          return m.ctx.audioWorklet.addModule(worklet).then(function () {
            if (!m.ctx) return;
            var src = m.ctx.createMediaStreamSource(media);
            m.node = new AudioWorkletNode(m.ctx, "galion-pcm16", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 1, channelCountMode: "explicit" });
            m.node.port.onmessage = function (e) {
              if (!m.stopping) micLevel(e.data);
              if (m.ws && m.ws.readyState === 1 && !m.stopping) m.ws.send(e.data);
            };
            src.connect(m.node);
            openSocket();
          });
        }, function (err) {
          var name = err && err.name;
          if (name === "NotAllowedError" || name === "SecurityError") { cleanup("blocked"); setNote("Microphone access is blocked for this site. Allow it in the browser settings, then tap Mic blocked to retry.", true); }
          else if (name === "NotFoundError" || name === "OverconstrainedError") { cleanup("idle"); setNote("No microphone was found on this device.", true); }
          else { cleanup("idle"); setNote("The microphone could not start.", true); }
          return null;
        });
      }).catch(function () { cleanup("idle"); setNote("Dictation could not start in this browser.", true); });
    }
    function stopMic() {
      if (!m.ws) { cleanup("idle"); return; }
      m.stopping = true;
      releaseAudio();
      if (m.ws.readyState === 1) { m.ws.send(JSON.stringify({ type: "stop" })); show("finishing"); }
      // galion-stt sends the final line and closes; do not wait forever.
      m.timer = setTimeout(function () { cleanup("idle"); setNote(readyNote()); }, 4000);
      setNote(readyNote());
    }
    btn.addEventListener("click", function () {
      if (m.state === "live" || m.state === "connecting") stopMic();
      else if (m.state === "idle" || m.state === "blocked" || m.state === "offline") start();
    });
    window.HermesUI.mic = { state: function () { return m.state; }, stop: stopMic };
    if (!worklet || !(navigator.mediaDevices && window.AudioWorkletNode && window.WebSocket)) { show("unsupported"); return; }
    show("checking");
    health().then(function (ok) { if (m.state === "checking") show(ok ? "idle" : "offline"); });
  })();
})();
