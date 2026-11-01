// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Galion One service worker, switched off (ONE_PWA=off): drop the shell
// cache and unregister, so every request goes to the network again.
"use strict";
self.addEventListener("install", function () { self.skipWaiting(); });
self.addEventListener("activate", function (e) {
  e.waitUntil(
    caches.keys().then(function (keys) {
      return Promise.all(keys.filter(function (k) {
        return k.indexOf("g1-shell-") === 0;
      }).map(function (k) { return caches.delete(k); }));
    }).then(function () { return self.registration.unregister(); })
  );
});
