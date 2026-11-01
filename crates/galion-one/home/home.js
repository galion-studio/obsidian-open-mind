// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// galion.app UI sounds: synthesized with Web Audio, no files. Off by default; nothing plays until the visitor turns sound on.
(function () {
  var b = document.getElementById("snd"), A = window.AudioContext || window.webkitAudioContext;
  if (!b || !A) return;
  b.hidden = false;
  var on = false, c = null, calm = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
  function tone(f, d, v) {
    if (!on || !c || c.state !== "running") return;
    try {
      var t = c.currentTime, o = c.createOscillator(), g = c.createGain();
      o.type = "sine"; o.frequency.setValueAtTime(f, t); o.frequency.exponentialRampToValueAtTime(f * 1.5, t + d);
      g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(v, t + 0.012); g.gain.exponentialRampToValueAtTime(0.0001, t + d);
      o.connect(g); g.connect(c.destination); o.start(t); o.stop(t + d + 0.03);
    } catch (e) {}
  }
  b.addEventListener("click", function () {
    on = !on;
    b.setAttribute("aria-pressed", on ? "true" : "false");
    b.querySelector(".lbl").textContent = on ? "Sound on" : "Sound off";
    if (!on) return;
    try {
      if (!c) c = new A();
      var r = c.state === "suspended" ? c.resume() : null;
      if (r && r.then) r.then(function () { tone(520, 0.2, 0.05); }, function () {}); else tone(520, 0.2, 0.05);
    } catch (e) {}
  });
  function pick(e) { return e.target && e.target.closest ? e.target.closest("a,.btn") : null; }
  document.addEventListener("click", function (e) { var a = pick(e); if (a && a !== b) tone(660, 0.09, 0.04); });
  if (!calm) document.addEventListener("pointerover", function (e) {
    var a = pick(e);
    if (a && e.pointerType === "mouse" && !(e.relatedTarget && a.contains(e.relatedTarget))) tone(880, 0.05, 0.012);
  });
})();
// Plan buttons (t550u): only when the server sets data-checkout="probe" (ONE_CHECKOUT=probe), ask galion-accounts whether checkout is connected. HEAD only, so no checkout
// session is created; only a 2xx/3xx answer turns "Coming soon" into a link. Anything else
// ("not connected", 404, offline) leaves "Coming soon" in place.
(function () {
  if (!window.fetch || document.documentElement.getAttribute("data-checkout") !== "probe") return;
  var soon = document.querySelectorAll("[data-plan]");
  for (var i = 0; i < soon.length; i++) (function (el) {
    var plan = el.getAttribute("data-plan");
    if (plan !== "plus" && plan !== "pro") return;
    var url = "/api/checkout?plan=" + plan;
    fetch(url, { method: "HEAD", cache: "no-store", credentials: "same-origin", redirect: "manual" }).then(function (r) {
      if (!(r.type === "opaqueredirect" || (r.status >= 200 && r.status < 400))) return;
      var a = document.createElement("a");
      a.className = "btn pri"; a.href = url; a.textContent = "Subscribe";
      el.parentNode.replaceChild(a, el);
      var n = document.getElementById("paysoon"); if (n) n.hidden = true;
    }, function () {});
  })(soon[i]);
})();
