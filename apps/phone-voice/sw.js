// SPDX-License-Identifier: MIT
// Minimal service worker: caches the app shell so the PWA installs and opens
// offline. Never caches config.json or relay traffic.
const C = "omvoice-v0.1";
const SHELL = ["./", "index.html", "app.js", "manifest.webmanifest", "icon.svg"];
self.addEventListener("install", (e) => e.waitUntil(caches.open(C).then((c) => c.addAll(SHELL))));
self.addEventListener("activate", (e) => e.waitUntil(caches.keys().then((k) => Promise.all(k.filter((x) => x !== C).map((x) => caches.delete(x))))));
self.addEventListener("fetch", (e) => {
  const u = new URL(e.request.url);
  if (e.request.method !== "GET" || u.origin !== location.origin || u.pathname.endsWith("config.json")) return;
  e.respondWith(caches.match(e.request).then((r) => r || fetch(e.request)));
});
