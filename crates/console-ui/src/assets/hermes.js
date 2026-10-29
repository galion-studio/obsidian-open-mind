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
    mode = $("hm-mode"), model = $("hm-model"), modelWrap = $("hm-model-wrap"),
    status = $("hm-status");
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
  function syncModel() {
    if (modelWrap) modelWrap.hidden = provider() !== "openrouter";
  }
  function canSend() {
    return !state.busy && q.value.trim().length > 0 && provider() !== "" && state.spendLevel !== "blocked";
  }
  function refreshButtons() {
    send.disabled = !canSend();
    stop.hidden = !state.busy;
    log.setAttribute("aria-busy", state.busy ? "true" : "false");
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
  function msg(kind, text) {
    var a = el("article", "hm-msg hm-" + kind);
    a.appendChild(el("p", "hm-who", kind === "user" ? "You" : "Hermes"));
    var t = el("div", "hm-text");
    renderText(t, text || "");
    a.appendChild(t);
    log.appendChild(a);
    return t;
  }
  function tool(name) { var d = el("div", "hm-tool"); d.appendChild(el("span", "mono", name)); log.appendChild(d); return d; }
  function boxMsg(cls, text) { var d = el("div", cls, text); log.appendChild(d); return d; }
  function scrollEnd() { log.scrollTop = log.scrollHeight; }
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

  // Asking -------------------------------------------------------------
  function finish(text) {
    state.busy = false;
    state.run = "";
    if (state.es) { try { state.es.close(); } catch (e) { /* closed */ } state.es = null; }
    refreshButtons();
    setNote(text || readyNote(), false);
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
        if (r.status === "completed") ended(function () { if (!got && r.output) renderText(out, r.output); finish("Done."); });
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
          got += e.delta || ""; renderText(out, got); scrollEnd(); break;
        case "tool.started":
          if (pending) pending.textContent = "Using " + (e.tool || "a tool") + ".";
          log.insertBefore(el("div", "hm-tool", "tool · " + (e.tool || "tool")), out.parentNode);
          break;
        case "status":
          if (pending && e.text) pending.textContent = String(e.text).slice(0, 160);
          break;
        case "run.completed":
          ended(function () { if (!got && e.output) renderText(out, e.output); scrollEnd(); finish("Done."); });
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
    if (provider() === "openrouter" && model && model.value) body.model = model.value;
    if (state.thread) body.session_id = state.thread;
    var w = $("hm-welcome"); if (w && w.parentNode) w.parentNode.removeChild(w);
    Array.prototype.forEach.call(log.querySelectorAll(".hm-welcome"), function (n) { n.parentNode.removeChild(n); });
    msg("user", text);
    var out = msg("assistant", "");
    var pending = el("p", "hm-pending", "Asking Hermes. Research can take a few minutes.");
    out.parentNode.appendChild(pending);
    scrollEnd();
    q.value = "";
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
  q.addEventListener("input", refreshButtons);
  send.addEventListener("click", ask);
  stop.addEventListener("click", stopRun);
  root.addEventListener("change", function (e) {
    if (e.target && e.target.name === "hm-provider") { syncModel(); refreshButtons(); }
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
    var id = (e.state && e.state.id) || new URLSearchParams(location.search).get("id") || "";
    if (id) openThread(id, false); else newThread(false);
  });
  document.addEventListener("keydown", function (e) {
    if (e.key === "/" && document.activeElement !== q && document.activeElement !== search) { e.preventDefault(); search.focus(); }
    if (e.key === "Escape" && state.busy) stopRun();
  });
  syncModel();
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
      btn.textContent = l[0];
      btn.title = detail || l[1];
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
