// Galion One V2 room (owner orders t431u/t432u, 2026-10-30 CET), ported from the console V2 /hermes room (68d5711e hermes.js).
// Plain ES5-style script, no framework, no build step. Talks only to the galion-one routes of this host:
// /me /pair /pair/invite /admin/say /threads /session /files /model /models /voice /api/v1/awake /stt/health /stt/stream.
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var body = document.body, hm = $("hm"), log = $("hm-log"), q = $("hm-q"), send = $("hm-send"), note = $("hm-note");
  var cap = $("g1-cap"), mic = $("hm-mic"), partial = $("hm-partial"), form = $("hm-form"), pairForm = $("g1-pair");
  var NOTE = "Enter sends. Shift+Enter adds a line.";
  var paired = false, busy = false, chatting = false;

  function api(path, opts) {
    opts = opts || {};
    opts.credentials = "same-origin";
    opts.cache = "no-store";
    return fetch(path, opts).then(function (r) {
      return r.text().then(function (t) {
        var j = null;
        try { j = JSON.parse(t); } catch (e) { j = null; }
        return { status: r.status, json: j || {} };
      });
    });
  }
  function post(path, data) {
    return api(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(data || {}) });
  }

  // ---- State: drives the orb, the sky and the caption.
  var CAPS = { idle: "Ready", listen: "Listening", think: "Thinking", speak: "Answering", error: "Something went wrong" };
  var errTimer = 0;
  function setState(s, text) {
    clearTimeout(errTimer);
    body.setAttribute("data-state", s);
    cap.textContent = text || CAPS[s] || "";
    if (s === "error") errTimer = setTimeout(function () { setState("idle"); }, 4000);
  }

  // ---- Greeting: Zagreb hour, words rise in one by one.
  function zagrebHour() {
    try { return parseInt(new Intl.DateTimeFormat("en-GB", { hour: "numeric", hour12: false, timeZone: "Europe/Zagreb" }).format(new Date()), 10); }
    catch (e) { return new Date().getHours(); }
  }
  function words(el, text, start) {
    el.textContent = "";
    var parts = text.split(" ");
    for (var i = 0; i < parts.length; i++) {
      var w = document.createElement("span");
      w.className = "g1-w";
      w.style.animationDelay = (start + i * 0.09).toFixed(2) + "s";
      w.textContent = parts[i];
      el.appendChild(w);
      if (i < parts.length - 1) el.appendChild(document.createTextNode(" "));
    }
    return start + parts.length * 0.09;
  }
  (function greet() {
    var h = zagrebHour();
    var hi = h < 5 ? "Good night." : h < 12 ? "Good morning." : h < 18 ? "Good afternoon." : "Good evening.";
    // The room glides in as the loading screen lifts (about 2 s); the words start then.
    var t = words($("g1-hi1"), hi, 2.2);
    words($("g1-hi2"), "What shall we explore?", t + 0.1);
  })();

  // ---- Sound cues (off by default, remembered on this phone).
  var SND = "galion-one-sound", sound = false, actx = null;
  try { sound = localStorage.getItem(SND) === "on"; } catch (e) { sound = false; }
  function paintSound() { var b = $("g1-sound"); b.setAttribute("aria-pressed", sound ? "true" : "false"); b.textContent = sound ? "Sound on" : "Sound off"; }
  paintSound();
  $("g1-sound").addEventListener("click", function () {
    sound = !sound;
    try { localStorage.setItem(SND, sound ? "on" : "off"); } catch (e) {}
    paintSound();
    if (sound) cue("done");
  });
  function cue(kind) {
    if (!sound) return;
    try {
      var C = window.AudioContext || window.webkitAudioContext;
      if (!C) return;
      actx = actx || new C();
      var f = { send: [660, 880], reply: [520, 660], done: [880, 660], listen: [440, 660], error: [300, 220] }[kind] || [600, 600];
      var t = actx.currentTime, o = actx.createOscillator(), g = actx.createGain();
      o.type = "sine";
      o.frequency.setValueAtTime(f[0], t);
      o.frequency.exponentialRampToValueAtTime(f[1], t + 0.14);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.08, t + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.22);
      o.connect(g); g.connect(actx.destination);
      o.start(t); o.stop(t + 0.24);
    } catch (e) {}
  }

  // ---- Status line.
  function pill(id, cls, text) { var d = $(id), v = $(id + "-v"); d.className = "hm-dot" + (cls ? " " + cls : ""); v.textContent = text; }
  function refreshAwake() {
    api("/api/v1/awake").then(function (r) {
      var rows = r.json.services || [], mind = null;
      for (var i = 0; i < rows.length; i++) if (rows[i].id === "hermes") mind = rows[i];
      if (!mind) pill("st-mind", "warn", "unknown");
      else if (mind.ok) pill("st-mind", "ok", "awake");
      else pill("st-mind", "bad", "asleep");
    }).catch(function () { pill("st-mind", "bad", "offline"); });
  }

  // ---- Conversation.
  function enterChat() {
    if (chatting) return;
    chatting = true;
    hm.classList.add("g1-chat");
  }
  function addMsg(role, text) {
    enterChat();
    var m = document.createElement("div");
    m.className = "hm-msg g1-new " + (role === "user" ? "hm-user" : "hm-assistant");
    var who = document.createElement("p");
    who.className = "hm-who";
    who.textContent = role === "user" ? "You" : "Hermes";
    var t = document.createElement("div");
    t.className = "hm-text";
    t.textContent = text || "";
    m.appendChild(who); m.appendChild(t);
    log.appendChild(m);
    scrollEnd();
    return t;
  }
  function scrollEnd() { window.scrollTo(0, document.documentElement.scrollHeight); }
  function reveal(el, text, done) {
    var parts = text.split(/(\s+)/), i = 0;
    var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) { el.textContent = text; done(); return; }
    (function step() {
      if (i >= parts.length) { done(); return; }
      var s = document.createElement("span");
      s.className = "g1-c";
      s.textContent = parts.slice(i, i + 6).join("");
      el.appendChild(s);
      i += 6;
      scrollEnd();
      setTimeout(step, 60);
    })();
  }
  function setBusy(b) { busy = b; send.disabled = b || !q.value.trim(); }
  function ask(text) {
    text = (text || "").trim();
    if (!text || busy || !paired) return;
    stopMic();
    addMsg("user", text);
    q.value = ""; grow();
    setBusy(true);
    setState("think");
    cue("send");
    var wait = document.createElement("p");
    wait.className = "hm-pending";
    wait.textContent = "Hermes is thinking";
    log.appendChild(wait);
    scrollEnd();
    var pick = threadFor();
    post("/admin/say", { text: text, thread: pick.thread, fresh: pick.fresh }).then(function (r) {
      if (wait.parentNode) wait.parentNode.removeChild(wait);
      if (r.status === 401 || r.json.error === "pair") { setBusy(false); unpaired("This phone is no longer paired. Enter your pairing code."); return; }
      var say = (r.json.say || "").trim();
      if (r.json.thread) {
        var was = getThread();
        setThread({ id: r.json.thread, last: Date.now(), pinned: was.id === r.json.thread && !!was.pinned, fresh: false });
      }
      if (say === "Wait") return fail("Hermes is still answering something else. Try again in a moment.");
      if (!say || say === "Again") return fail("Hermes did not answer. Try again.");
      setState("speak");
      cue("reply");
      reveal(addMsg("assistant", ""), say, function () { setBusy(false); setState("idle"); cue("done"); loadHistory(); });
    }).catch(function () {
      if (wait.parentNode) wait.parentNode.removeChild(wait);
      fail("No connection. Check your network and try again.");
    });
  }
  // ---- Threads (t443u/t444u): one conversation = one Hermes thread.
  // The active thread is kept on this phone with the time of its last turn.
  // After 30 idle minutes the next message starts a new thread, unless the
  // user reopened this thread from History (pinned) -- then it stays open
  // until they tap New chat or open another one.
  var TH = "galion-one-thread", IDLE_MS = 30 * 60 * 1000;
  function getThread() {
    try { var t = JSON.parse(localStorage.getItem(TH) || "null"); if (t && typeof t === "object") return t; } catch (e) {}
    return { id: "", last: 0, pinned: false, fresh: false };
  }
  function setThread(t) { try { localStorage.setItem(TH, JSON.stringify(t)); } catch (e) {} }
  function threadFor() {
    var t = getThread();
    if (t.fresh || !t.id) return { thread: "", fresh: true };
    if (!t.pinned && Date.now() - (t.last || 0) >= IDLE_MS) return { thread: "", fresh: true };
    return { thread: t.id, fresh: false };
  }
  function clearLog() { log.textContent = ""; }
  function newChat(quiet) {
    if (busy) return;
    setThread({ id: "", last: 0, pinned: false, fresh: true });
    clearLog();
    chatting = false; hm.classList.remove("g1-chat");
    $("hm-hist").open = false;
    markThread("");
    if (!quiet) { setState("idle", "New chat"); cue("done"); }
    q.focus();
  }
  function markThread(id) {
    Array.prototype.forEach.call($("hm-list").querySelectorAll("button.hm-item"), function (b) {
      if (id && b.getAttribute("data-id") === id) b.setAttribute("aria-current", "true"); else b.removeAttribute("aria-current");
    });
  }
  function openThread(id, pinned) {
    return api("/threads?id=" + encodeURIComponent(id)).then(function (r) {
      if (r.status !== 200) { if (pinned) fail("That conversation did not load. Try again."); return false; }
      clearLog();
      var msgs = r.json.messages || [];
      msgs.forEach(function (m) { var el = addMsg(m.role === "user" ? "user" : "assistant", ""); el.textContent = m.text || ""; });
      if (!msgs.length) { chatting = false; hm.classList.remove("g1-chat"); }
      var t = getThread();
      setThread({ id: id, last: pinned ? Date.now() : (t.last || Date.now()), pinned: pinned || (t.id === id && t.pinned), fresh: false });
      markThread(id);
      $("hm-hist").open = false;
      if (pinned) setState("idle", "Back in this conversation");
      scrollEnd();
      return true;
    });
  }
  $("g1-new").addEventListener("click", function () { newChat(false); });
  function resumeThread() {
    var t = getThread();
    if (!t.id || t.fresh) return;
    if (!t.pinned && Date.now() - (t.last || 0) >= IDLE_MS) return;
    openThread(t.id, false).catch(function () {});
  }
  function fail(msg) {
    var p = document.createElement("p");
    p.className = "hm-err g1-new";
    p.textContent = msg;
    log.appendChild(p);
    scrollEnd();
    setBusy(false);
    setState("error");
    cue("error");
  }

  // ---- Composer.
  function grow() {
    q.style.height = "auto";
    q.style.height = Math.min(q.scrollHeight, window.innerHeight * 0.3) + "px";
    send.disabled = busy || !q.value.trim();
  }
  q.addEventListener("input", grow);
  q.addEventListener("keydown", function (e) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); ask(q.value); }
  });
  form.addEventListener("submit", function (e) { e.preventDefault(); ask(q.value); });
  function say(text, bad) { note.textContent = text || NOTE; note.className = "hm-note" + (bad ? " t-bad" : ""); }

  // ---- History: this phone's Galion One conversations (one Hermes thread each).
  function when(sec) {
    if (!sec) return "";
    try { return new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", timeZone: "Europe/Zagreb" }).format(new Date(sec * 1000)); }
    catch (e) { return new Date(sec * 1000).toLocaleString(); }
  }
  function loadHistory() {
    if (!paired) return;
    api("/session").then(function (r) { if (r.json.voice) paintVoice(r.json.voice); }).catch(function () {});
    api("/threads").then(function (r) {
      if (r.status !== 200) return;
      var rows = r.json.threads || [], list = $("hm-list");
      list.textContent = "";
      if (!rows.length) { var e = document.createElement("li"); e.className = "hm-empty"; e.textContent = "Nothing yet."; list.appendChild(e); return; }
      rows.forEach(function (th) {
        var li = document.createElement("li"), b = document.createElement("button");
        b.type = "button"; b.className = "hm-item"; b.setAttribute("data-id", th.id);
        var a = document.createElement("span"); a.className = "hm-it"; a.textContent = th.title || th.id;
        var m = document.createElement("span"); m.className = "hm-im"; m.textContent = [when(th.at), (th.count || 0) + " messages"].filter(Boolean).join(" · ");
        b.appendChild(a); b.appendChild(m);
        b.addEventListener("click", function () { if (!busy) openThread(th.id, true).catch(function () { fail("No connection. Check your network and try again."); }); });
        li.appendChild(b); list.appendChild(li);
      });
      var t = getThread();
      markThread(t.fresh ? "" : t.id);
    }).catch(function () {});
  }

  // ---- Model (read-only: galion-one keeps one fixed model).
  function loadModel() {
    api("/model").then(function (r) {
      var id = r.json.model || "";
      $("hm-model-v").textContent = id ? id.split("/").pop() : "unknown";
      if (!id) return;
      api("/models").then(function (m) {
        var rows = Array.isArray(m.json) ? m.json : (m.json.data || []);
        for (var i = 0; i < rows.length; i++) if (rows[i] && rows[i].id === id && rows[i].name) { $("hm-model-v").textContent = String(rows[i].name).replace(/^[^:]+:\s*/, ""); return; }
      }).catch(function () {});
    }).catch(function () { $("hm-model-v").textContent = "unknown"; });
  }
  $("hm-model").addEventListener("click", function () { say("Galion One uses one fixed model. Change it on your Galion server."); setTimeout(function () { say(); }, 4000); });

  // ---- Voice.
  function paintVoice(v) {
    var bs = document.querySelectorAll("[data-voice]");
    for (var i = 0; i < bs.length; i++) bs[i].setAttribute("aria-pressed", bs[i].getAttribute("data-voice") === v ? "true" : "false");
  }
  Array.prototype.forEach.call(document.querySelectorAll("[data-voice]"), function (b) {
    b.addEventListener("click", function () {
      post("/voice", { voice: b.getAttribute("data-voice") }).then(function (r) { paintVoice(r.json.voice || b.getAttribute("data-voice")); }).catch(function () {});
    });
  });

  // ---- Files (replaces the old "+").
  function loadFiles() {
    api("/files").then(function (r) {
      var ul = $("g1-flist"), rows = r.json.files || [];
      ul.textContent = "";
      rows.slice(0, 12).forEach(function (f) {
        var li = document.createElement("li"), a = document.createElement("a");
        a.href = "/files/" + encodeURIComponent(f.id); a.textContent = f.name || "file";
        li.appendChild(a); ul.appendChild(li);
      });
    }).catch(function () {});
  }
  $("g1-files").addEventListener("toggle", function () { if (this.open) loadFiles(); });
  $("g1-add").addEventListener("click", function () { $("g1-file").click(); });
  $("g1-file").addEventListener("change", function () {
    var f = this.files && this.files[0];
    if (!f) return;
    var name = f.type && f.type.indexOf("image/") === 0 && (!f.name || f.name === "file") ? "photo.jpg" : (f.name || "file");
    say("Saving " + name);
    fetch("/files?name=" + encodeURIComponent(name), { method: "POST", body: f, credentials: "same-origin" }).then(function (r) { return r.json(); }).then(function (j) {
      say(j && j.id ? "Saved " + (j.name || name) : "Could not save that file.", !(j && j.id));
      loadFiles();
      setTimeout(function () { say(); }, 3000);
    }).catch(function () { say("Could not save that file.", true); });
    this.value = "";
  });

  // ---- Devices: a paired phone shows a one-time code for a new phone.
  var inviteTimer = 0;
  $("g1-invite").addEventListener("click", function () {
    post("/pair/invite").then(function (r) {
      var out = $("g1-invite-out");
      if (!r.json.code) { $("g1-invite-code").textContent = ""; $("g1-invite-hint").textContent = "Could not make a code. Try again."; out.hidden = false; return; }
      var until = new Date(r.json.expires), hh;
      try { hh = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: "Europe/Zagreb" }).format(until); } catch (e) { hh = until.toTimeString().slice(0, 5); }
      $("g1-invite-code").textContent = r.json.code;
      $("g1-invite-hint").textContent = "On the new phone open one.galion.app, type this code in Pairing code and tap Pair. Works once, until " + hh + " (Zagreb time).";
      var topic = /^[A-Za-z0-9_-]{1,64}$/.test(r.json.ntfy || "") ? r.json.ntfy : "";
      $("g1-ntfy").hidden = !topic;
      if (topic) {
        $("g1-ntfy-app").href = "ntfy://ntfy.sh/" + topic;
        $("g1-ntfy-web").href = "https://ntfy.sh/" + topic;
        $("g1-ntfy-topic").textContent = "ntfy.sh/" + topic;
      }
      out.hidden = false;
      clearTimeout(inviteTimer);
      inviteTimer = setTimeout(function () { $("g1-invite-code").textContent = ""; $("g1-ntfy-topic").textContent = ""; out.hidden = true; }, Math.max(1000, r.json.expires - Date.now()));
    }).catch(function () {});
  });

  // ---- Pairing.
  function unpaired(msg) {
    paired = false;
    form.hidden = true;
    pairForm.hidden = false;
    $("g1-threads").hidden = true;
    pill("st-dev", "warn", "not paired");
    pill("st-stt", "", "pair first");
    $("g1-pair-note").textContent = msg || "";
    setState("idle", "Pair this phone to talk");
  }
  // Approvals (2026-10-31 CET): how many Hermes self-changes wait for a signature; the screen is /approvals.
  function loadApprovals() {
    api("/api/approvals/count").then(function (r) { var n = r.json.ready || 0; if (n) { $("g1-appr-n").textContent = n + " waiting for you"; $("g1-dev").open = true; } }).catch(function () {});
  }
  function ready() {
    paired = true;
    pairForm.hidden = true;
    form.hidden = false;
    $("g1-threads").hidden = false;
    pill("st-dev", "ok", "paired");
    setState("idle");
    loadModel(); loadHistory(); sttHealth(); resumeThread();
    loadApprovals();
  }
  pairForm.addEventListener("submit", function (e) {
    e.preventDefault();
    var code = $("g1-code").value.trim();
    if (!code) return;
    $("g1-pair-note").textContent = "Pairing";
    post("/pair", { code: code }).then(function (r) {
      if (r.json.ok) { $("g1-code").value = ""; $("g1-pair-note").textContent = ""; ready(); cue("done"); return; }
      $("g1-pair-note").textContent = r.json.error === "later" ? "Too many tries. Wait 15 minutes, then try again." : "That code did not work.";
      setState("error", "Not paired");
    }).catch(function () { $("g1-pair-note").textContent = "No connection."; });
  });

  // ---- Dictation: AudioWorklet -> 16 kHz PCM16 frames -> /stt/stream (galion-one proxies to galion-stt).
  var MIC_LABEL = { checking: "Checking dictation", idle: "Dictate", connecting: "Starting dictation", live: "Stop dictation", finishing: "Finishing", blocked: "Microphone blocked", offline: "Dictation offline", unsupported: "Dictation not supported here" };
  var micState = "checking", ws = null, actxMic = null, node = null, src = null, streamIn = null, finalText = "", baseText = "", stopTimer = 0;
  function setMic(s) {
    micState = s;
    mic.setAttribute("data-state", s);
    mic.setAttribute("aria-pressed", s === "live" ? "true" : "false");
    mic.setAttribute("aria-label", MIC_LABEL[s] || "Dictate");
    mic.title = MIC_LABEL[s] || "Dictate";
    $("g1-orb").setAttribute("aria-label", s === "live" ? "Stop dictation" : "Start dictation");
  }
  var canMic = !!(window.AudioWorkletNode && navigator.mediaDevices && navigator.mediaDevices.getUserMedia && window.WebSocket && window.isSecureContext !== false);
  function sttHealth() {
    if (!canMic) { setMic("unsupported"); pill("st-stt", "", "not on this browser"); return; }
    api("/stt/health").then(function (r) {
      if (r.status === 401) { setMic("offline"); pill("st-stt", "", "pair first"); return; }
      if (r.json.ok) { if (micState === "checking" || micState === "offline") setMic("idle"); pill("st-stt", "ok", "on"); }
      else { if (micState !== "live") setMic("offline"); pill("st-stt", "bad", "offline"); }
    }).catch(function () { if (micState !== "live") setMic("offline"); pill("st-stt", "bad", "offline"); });
  }
  var workletUrl = null;
  function worklet() {
    if (workletUrl) return workletUrl;
    var src = $("g1-worklet").textContent;
    workletUrl = URL.createObjectURL(new Blob([src], { type: "application/javascript" }));
    return workletUrl;
  }
  function showPartial(text) {
    var t = (finalText + (text ? " " + text : "")).trim();
    partial.hidden = !t;
    partial.textContent = t;
  }
  function startMic() {
    if (!paired || busy) return;
    if (micState === "unsupported") { say("Dictation needs a newer browser.", true); return; }
    if (micState !== "idle" && micState !== "blocked" && micState !== "offline") return;
    setMic("connecting");
    finalText = "";
    baseText = q.value.trim();
    navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } }).then(function (stream) {
      streamIn = stream;
      var C = window.AudioContext || window.webkitAudioContext;
      actxMic = new C();
      return actxMic.audioWorklet.addModule(worklet()).then(function () {
        src = actxMic.createMediaStreamSource(stream);
        node = new AudioWorkletNode(actxMic, "galion-pcm16");
        var proto = location.protocol === "https:" ? "wss:" : "ws:";
        ws = new WebSocket(proto + "//" + location.host + "/stt/stream");
        ws.binaryType = "arraybuffer";
        ws.onopen = function () {
          setMic("live");
          setState("listen");
          say("Listening. Tap the mic to stop.");
          cue("listen");
          node.port.onmessage = function (e) {
            var buf = e.data;
            if (ws && ws.readyState === 1) ws.send(buf);
            var a = new Int16Array(buf), s = 0;
            for (var i = 0; i < a.length; i += 4) s += (a[i] / 32768) * (a[i] / 32768);
            var lvl = Math.min(1, Math.sqrt(s / (a.length / 4)) * 4);
            hm.style.setProperty("--lvl", lvl.toFixed(3));
            document.documentElement.style.setProperty("--lvl", lvl.toFixed(3));
          };
          src.connect(node);
        };
        ws.onmessage = function (e) {
          var m = null;
          try { m = JSON.parse(e.data); } catch (x) { return; }
          if (m.type === "partial") showPartial(m.text || "");
          else if (m.type === "final") { if (m.text) finalText = (finalText + " " + m.text).trim(); showPartial(""); }
          else if (m.type === "error") { say("Dictation: " + (m.text || m.error || "error"), true); }
        };
        ws.onerror = function () {};
        ws.onclose = function () { finishMic(micState === "connecting"); };
      });
    }).catch(function (err) {
      cleanup();
      var blocked = err && (err.name === "NotAllowedError" || err.name === "SecurityError");
      setMic(blocked ? "blocked" : "offline");
      setState("error", blocked ? "Microphone blocked" : "Dictation offline");
      say(blocked ? "Allow the microphone for one.galion.app in Chrome settings, then tap the mic again." : "Dictation could not start.", true);
    });
  }
  function stopMic() {
    if (micState !== "live" && micState !== "connecting") return;
    setMic("finishing");
    if (node) { try { node.port.postMessage("stop"); } catch (e) {} }
    if (src) { try { src.disconnect(); } catch (e) {} }
    if (ws && ws.readyState === 1) { try { ws.send(JSON.stringify({ type: "stop" })); } catch (e) {} }
    stopTimer = setTimeout(function () { if (ws) { try { ws.close(); } catch (e) {} } finishMic(false); }, 2500);
  }
  function cleanup() {
    if (streamIn) { streamIn.getTracks().forEach(function (t) { t.stop(); }); streamIn = null; }
    if (actxMic) { try { actxMic.close(); } catch (e) {} actxMic = null; }
    node = null; src = null;
    hm.style.setProperty("--lvl", "0");
    document.documentElement.style.setProperty("--lvl", "0");
  }
  function finishMic(failed) {
    clearTimeout(stopTimer);
    if (micState === "idle" || micState === "offline" || micState === "blocked") return;
    ws = null;
    cleanup();
    var text = finalText.trim() || (partial.hidden ? "" : partial.textContent.trim());
    partial.hidden = true; partial.textContent = "";
    if (failed) { setMic("offline"); setState("error", "Dictation offline"); say("Dictation is offline right now. You can still type.", true); sttHealth(); return; }
    setMic("idle");
    if (!busy) setState("idle");
    if (text) { q.value = (baseText ? baseText + " " : "") + text; grow(); q.focus(); say("Check the words, then send."); }
    else say();
  }
  mic.addEventListener("click", function () { if (micState === "live" || micState === "connecting") stopMic(); else startMic(); });
  $("g1-orb").addEventListener("click", function () { if (!paired) { $("g1-code").focus(); return; } if (micState === "live" || micState === "connecting") stopMic(); else startMic(); });
  document.addEventListener("visibilitychange", function () { if (document.hidden) stopMic(); });

  // ---- Start.
  setState("idle", "Waking up");
  refreshAwake();
  setInterval(refreshAwake, 30000);
  setInterval(function () { if (paired && micState !== "live") sttHealth(); }, 60000);
  api("/me").then(function (r) { if (r.json.you) ready(); else unpaired(""); }).catch(function () { unpaired("No connection."); });
})();
