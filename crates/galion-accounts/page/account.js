// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// /account: create a number, log in, see the plan, start a payment. No libraries.
(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var state = { methods: null };

  function note(text, bad) {
    var n = $("notice");
    n.textContent = text;
    n.className = "notice" + (bad ? " bad" : "");
    n.hidden = !text;
  }

  function api(method, path, body) {
    var opt = { method: method, credentials: "same-origin", headers: {} };
    if (body !== undefined) {
      opt.headers["content-type"] = "application/json";
      opt.body = JSON.stringify(body);
    }
    return fetch(path, opt).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (j) { j._status = r.status; return j; });
    });
  }

  function show(which) {
    $("out").hidden = which !== "out";
    $("fresh").hidden = which !== "fresh";
    $("in").hidden = which !== "in";
    $("logout").hidden = which !== "in";
  }

  var LABEL = { stripe: "Card (Stripe)", crypto: "Crypto", cash: "Cash by post" };

  function paint(me) {
    state.methods = me.methods || {};
    $("plan").textContent = me.plan_name || "Free";
    var u = $("until");
    if (me.active && me.paid_until_label) {
      u.textContent = (me.renewing ? "Renews by card. Paid until " : "Paid until ") + me.paid_until_label + " (Zagreb time).";
    } else if (me.paid_until_label) {
      u.textContent = "Paid time ended " + me.paid_until_label + ".";
    } else {
      u.textContent = "No paid time on this account yet.";
    }
    $("manage").hidden = !me.can_manage;
    var list = $("methods");
    list.textContent = "";
    ["stripe", "crypto", "cash"].forEach(function (m) {
      var st = state.methods[m] || {};
      var li = document.createElement("li");
      li.className = st.connected ? "on" : "";
      li.textContent = LABEL[m] + ": " + (st.connected ? "ready" : (st.note || "not connected yet"));
      list.appendChild(li);
    });
    var btns = document.querySelectorAll(".pay");
    for (var i = 0; i < btns.length; i++) {
      var b = btns[i];
      var st2 = state.methods[b.getAttribute("data-m")] || {};
      var blocked = b.getAttribute("data-m") === "stripe" && me.renewing;
      b.disabled = !st2.connected || blocked;
      b.title = st2.connected ? "" : (st2.note || "Not connected yet");
    }
  }

  function refresh() {
    return api("GET", "/api/account/me").then(function (me) {
      if (me._status === 200 && me.ok) { paint(me); show("in"); }
      else { show("out"); }
    }).catch(function () { show("out"); note("Could not reach the account service. Try again.", true); });
  }

  $("create").addEventListener("click", function () {
    var b = this; b.disabled = true;
    api("POST", "/api/account/create", {}).then(function (j) {
      b.disabled = false;
      if (!j.ok) { note(j.message || "Could not create an account.", true); return; }
      note("");
      $("freshnum").textContent = j.number;
      $("saved").checked = false; $("cont").disabled = true;
      show("fresh");
    }).catch(function () { b.disabled = false; note("Could not reach the account service.", true); });
  });

  $("copy").addEventListener("click", function () {
    var t = $("freshnum").textContent.replace(/\s/g, "");
    if (navigator.clipboard) navigator.clipboard.writeText(t).then(function () { note("Copied. Paste it into your password manager now."); });
  });
  $("saved").addEventListener("change", function () { $("cont").disabled = !this.checked; });
  $("cont").addEventListener("click", function () { $("freshnum").textContent = ""; refresh(); });

  $("num").addEventListener("input", function () {
    var d = this.value.replace(/\D/g, "").slice(0, 16);
    this.value = d.replace(/(\d{4})(?=\d)/g, "$1 ");
  });

  $("login").addEventListener("submit", function (e) {
    e.preventDefault();
    var v = $("num").value;
    if (v.replace(/\D/g, "").length !== 16) { note("An account number has 16 digits.", true); return; }
    api("POST", "/api/account/login", { number: v }).then(function (j) {
      if (!j.ok) { note(j.message || "Login failed.", true); return; }
      $("num").value = ""; note(""); refresh();
    }).catch(function () { note("Could not reach the account service.", true); });
  });

  $("logout").addEventListener("click", function () {
    api("POST", "/api/account/logout", {}).then(function () { note("Logged out."); show("out"); });
  });

  $("manage").addEventListener("click", function () {
    api("POST", "/api/pay/stripe/portal", {}).then(function (j) {
      if (j.ok && j.url) location.href = j.url; else note(j.message || "Could not open the card portal.", true);
    });
  });

  document.addEventListener("click", function (e) {
    var b = e.target.closest ? e.target.closest(".pay") : null;
    if (!b || b.disabled) return;
    var plan = b.closest("[data-plan]").getAttribute("data-plan");
    var m = b.getAttribute("data-m");
    if (m === "stripe") {
      api("POST", "/api/pay/stripe/checkout", { plan: plan }).then(function (j) {
        if (j.ok && j.url) location.href = j.url; else note(j.message || "Card payment is not available.", true);
      });
    } else if (m === "cash") {
      api("POST", "/api/pay/cash", { plan: plan }).then(function (j) {
        if (!j.ok) { note(j.message || "Cash is not available.", true); return; }
        $("cashref").textContent = j.reference;
        $("cashtext").textContent = j.instructions;
        $("cashbox").hidden = false;
        $("cashbox").scrollIntoView({ behavior: "smooth", block: "start" });
      });
    } else {
      note("Crypto is not connected yet.", true);
    }
  });

  var q = location.search;
  if (q.indexOf("checkout=done") >= 0) note("Thank you. Your time appears as soon as Stripe confirms the payment, usually within seconds. Reload if it does not show yet.");
  if (q.indexOf("checkout=cancelled") >= 0) note("Payment cancelled. Nothing was charged.");
  if (q.indexOf("checkout=error") >= 0) note("Card checkout could not start. Try again in a minute.", true);
  var wanted = /[?&]plan=(plus|pro)/.exec(q);
  if (wanted && !q.match(/checkout=/)) {
    var nm = wanted[1] === "pro" ? "Pro (€220 / month)" : "Plus (€20 / month)";
    note("You picked " + nm + ". Create a number or log in, then choose how to pay below.");
  }
  refresh();
})();
