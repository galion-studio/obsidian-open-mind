// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Galion One service worker (home-screen install, 2026-11-01 CET).
// Caches only the app shell so the installed app opens offline.
// It never caches or answers API, pairing, auth, files, voice or approvals
// requests: anything not listed in SHELL goes straight to the network,
// untouched, and only GET requests are ever looked at.
"use strict";
var V = "g1-shell-v3";
var PAGE = "/";
var SHELL = [
  PAGE,
  "/manifest.webmanifest",
  "/pwa/icon.svg",
  "/pwa/icon-192.png",
  "/pwa/icon-512.png",
  "/pwa/maskable-512.png",
  "/apple-touch-icon.png"
];

self.addEventListener("install", function (e) {
  e.waitUntil(
    caches.open(V).then(function (c) {
      return Promise.all(SHELL.map(function (u) {
        return fetch(u, { cache: "reload", credentials: "omit" }).then(function (r) {
          if (r.ok) return c.put(u, r);
        }).catch(function () {});
      }));
    }).then(function () { return self.skipWaiting(); })
  );
});

self.addEventListener("activate", function (e) {
  e.waitUntil(
    caches.keys().then(function (keys) {
      return Promise.all(keys.filter(function (k) {
        return k.indexOf("g1-shell-") === 0 && k !== V;
      }).map(function (k) { return caches.delete(k); }));
    }).then(function () { return self.clients.claim(); })
  );
});

self.addEventListener("fetch", function (e) {
  var r = e.request;
  if (r.method !== "GET") return;
  var u = new URL(r.url);
  if (u.origin !== self.location.origin) return;
  // The room page: network first (always fresh when online), the cached
  // shell only when the network fails. Other pages (/approvals, /link,
  // /admin ...) are never cached.
  if (r.mode === "navigate") {
    if (u.pathname !== "/" && u.pathname !== "/index.html") return;
    e.respondWith(
      fetch(r).then(function (res) {
        if (res.ok && res.type === "basic") {
          var copy = res.clone();
          caches.open(V).then(function (c) { return c.put(PAGE, copy); });
        }
        return res;
      }).catch(function () {
        return caches.match(PAGE).then(function (m) { return m || Response.error(); });
      })
    );
    return;
  }
  // Shell files (icons, manifest): cache first. Everything else: untouched.
  if (u.pathname === PAGE || SHELL.indexOf(u.pathname) < 0) return;
  e.respondWith(
    caches.match(u.pathname).then(function (m) { return m || fetch(r); })
  );
});
