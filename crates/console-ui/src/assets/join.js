// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// /join (console-instances): adds this browser's readings to the join form
// and, once joined, reports them every 30 s while the page is open.
// No libraries. Works in Safari, Chrome and Firefox; without it the form
// still joins (just with no readings).
(function () {
  "use strict";
  function gpuName() {
    try {
      var c = document.createElement("canvas");
      var gl = c.getContext("webgl") || c.getContext("experimental-webgl");
      if (!gl) return "";
      var ext = gl.getExtension("WEBGL_debug_renderer_info");
      var n = ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
      return String(n || "").slice(0, 120);
    } catch (e) {
      return "";
    }
  }
  function osName() {
    var u = navigator.userAgent || "";
    if (/iPhone|iPad|iPod/.test(u)) return "iOS";
    if (/Android/.test(u)) return "Android";
    if (/Mac OS X/.test(u)) return "macOS";
    if (/Windows/.test(u)) return "Windows";
    if (/CrOS/.test(u)) return "ChromeOS";
    if (/Linux/.test(u)) return "Linux";
    return "";
  }
  function reading() {
    var m = { os: osName(), cpu_model: "", gpus: [], models: [] };
    if (navigator.hardwareConcurrency) m.cores = navigator.hardwareConcurrency;
    // deviceMemory is GB, rounded and capped by the browser (Chrome only).
    if (navigator.deviceMemory) m.ram_total_mb = navigator.deviceMemory * 1024;
    var g = gpuName();
    if (g) m.gpus.push({ name: g });
    return m;
  }
  var field = document.getElementById("fl-m");
  if (field) field.value = JSON.stringify(reading());
  var st = document.getElementById("fl-st");
  if (!st) return;
  function beat() {
    var x = new XMLHttpRequest();
    x.open("POST", "/join/beat");
    x.setRequestHeader("Content-Type", "application/json");
    x.onload = function () {
      var t = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      st.textContent = x.status === 200
        ? "Online. Last report " + t + "."
        : x.status === 401
          ? "This device was removed on the console. Get a new code to add it again."
          : "The console did not take the report (" + x.status + "). Trying again soon.";
    };
    x.onerror = function () {
      st.textContent = "No connection to the console. Trying again soon.";
    };
    x.send(JSON.stringify(reading()));
  }
  beat();
  setInterval(beat, 30000);
})();
