// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Approvals screen (approval-system C, 2026-10-31 CET). Private keys stay on this device:
// a passkey lives in the authenticator; an Ed25519 key file lives in IndexedDB as a
// non-extractable CryptoKey. The server only ever gets public keys and signatures.
(function () {
  'use strict';
  var $ = function (id) { return document.getElementById(id); };
  var te = new TextEncoder();
  var skew = 0, list = null, keyst = null, chals = {}, setup = null;

  function api(path, body) {
    var o = { credentials: 'same-origin', cache: 'no-store', headers: {} };
    if (body !== undefined) { o.method = 'POST'; o.headers['content-type'] = 'application/json'; o.body = JSON.stringify(body); }
    return fetch(path, o).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (j) {
        if (r.status === 401) { $('ap-unpaired').hidden = false; throw new Error('This device is not paired.'); }
        if (!r.ok) throw new Error(j.error || ('HTTP ' + r.status));
        return j;
      });
    });
  }
  function el(tag, cls, text) { var e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
  function hex(buf) { return Array.prototype.map.call(new Uint8Array(buf), function (b) { return ('0' + b.toString(16)).slice(-2); }).join(''); }
  function b64u(buf) { var s = '', a = new Uint8Array(buf); for (var i = 0; i < a.length; i++) s += String.fromCharCode(a[i]); return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''); }
  function unb64u(s) { s = s.replace(/-/g, '+').replace(/_/g, '/'); while (s.length % 4) s += '='; var b = atob(s), a = new Uint8Array(b.length); for (var i = 0; i < b.length; i++) a[i] = b.charCodeAt(i); return a; }
  function group4(h) { return (h || '').replace(/(.{4})/g, '$1 ').trim(); }
  function sha(buf) { return crypto.subtle.digest('SHA-256', buf).then(hex); }
  function nowS() { return Date.now() / 1000 + skew; }
  function note(id, msg, bad) { var n = $(id); n.textContent = msg || ''; n.className = 'hm-note' + (bad ? ' t-bad' : ''); }
  function dot(ok, warn) { return el('span', 'hm-dot ' + (ok ? 'ok' : warn ? 'warn' : 'bad')); }
  function hexBlock(h) {
    var p = el('p', 'ap-hex'), g = group4(h).split(' ');
    g.forEach(function (x, i) { var s = el(i < 4 ? 'b' : 'span', null, x); p.appendChild(s); if (i < g.length - 1) p.appendChild(document.createTextNode(' ')); });
    return p;
  }

  // ---- device keys (IndexedDB) ----
  function db() {
    return new Promise(function (ok, no) {
      var r = indexedDB.open('galion-owner-keys', 1);
      r.onupgradeneeded = function () { r.result.createObjectStore('k', { keyPath: 'fp' }); };
      r.onsuccess = function () { ok(r.result); }; r.onerror = function () { no(r.error); };
    });
  }
  function dbDo(mode, f) { return db().then(function (d) { return new Promise(function (ok, no) { var t = d.transaction('k', mode), q = f(t.objectStore('k')); t.oncomplete = function () { ok(q && q.result); }; t.onerror = function () { no(t.error); }; }); }); }
  function keyGet(fp) { return dbDo('readonly', function (s) { return s.get(fp); }); }
  function keyPut(rec) { return dbDo('readwrite', function (s) { return s.put(rec); }); }

  // Sign `domain \n body` with the device key the server names (passkey or key file).
  function signWith(kv, domain, body, chal) {
    if (kv.scheme === 'webauthn-es256') {
      var pk = { challenge: unb64u(chal), rpId: kv.passkey.rp_id, userVerification: 'required', timeout: 120000 };
      if (kv.passkey.credential_id) pk.allowCredentials = [{ type: 'public-key', id: unb64u(kv.passkey.credential_id) }];
      return navigator.credentials.get({ publicKey: pk }).then(function (c) {
        if (!c) throw new Error('No passkey answered.');
        return { scheme: 'webauthn-es256', credential_id: b64u(c.rawId), authenticator_data: b64u(c.response.authenticatorData),
          client_data_json: b64u(c.response.clientDataJSON), signature: b64u(c.response.signature), key_id: kv.key_id };
      });
    }
    return keyGet(kv.fingerprint).then(function (rec) {
      if (!rec) throw new Error('This device does not hold the key ' + kv.short + '. Use the device that made it, or load its key file.');
      return crypto.subtle.sign({ name: 'Ed25519' }, rec.priv, te.encode(domain + '\n' + body));
    }).then(function (sig) { return { scheme: 'ed25519', sig: hex(sig), key_id: kv.key_id }; });
  }

  // ---- approval cards ----
  function badge(c) {
    var m = { ready: ['go', 'Waiting for you'], approved: ['ok', 'Approved'], rejected: ['bad', 'Rejected'], deployed: ['ok', 'Deployed'],
      waiting: ['warn', 'Still testing'], blocked: ['bad', 'Blocked'], broken: ['bad', 'Unreadable'] }[c.state] || ['', c.state];
    return el('span', 'ap-badge ' + m[0], m[1]);
  }
  function chip(ok, warn, text) { var li = el('li', 'ap-chip'); li.appendChild(dot(ok, warn)); li.appendChild(document.createTextNode(text)); return li; }
  function more(title, open) { var d = el('details', 'ap-more'); d.appendChild(el('summary', null, title)); if (open) d.open = true; return d; }
  function diffView(text, cut) {
    var pre = el('pre', 'ap-diff');
    (text || '(no diff)').split('\n').forEach(function (l) {
      var k = /^(\+\+\+|---|diff --git)/.test(l) ? 'f' : l[0] === '+' ? 'a' : l[0] === '-' ? 'd' : l.slice(0, 2) === '@@' ? 'h' : '';
      pre.appendChild(el('span', k, l || ' '));
    });
    if (cut) pre.appendChild(el('span', 'h', '… diff cut at 64 KB; the signed patch is the full one.'));
    return pre;
  }
  function rows(items, f) { var ul = el('ul', 'ap-rows'); (items || []).forEach(function (x) { var li = el('li'); li.appendChild(dot(x.ok === true, false)); li.appendChild(el('span', null, f(x))); ul.appendChild(li); }); return ul; }
  function mmss(s) { s = Math.max(0, Math.floor(s)); return Math.floor(s / 60) + ':' + ('0' + (s % 60)).slice(-2); }

  function card(c) {
    var art = el('article', 'ap-card'); art.dataset.state = c.state; art.dataset.id = c.id;
    var top = el('div', 'ap-top');
    top.appendChild(el('p', 'ap-k', 'Request ' + c.id)); top.appendChild(badge(c)); art.appendChild(top);
    art.appendChild(el('p', 'ap-plain', c.plain || ''));
    if (c.state === 'broken') { art.appendChild(el('p', 'ap-out t-bad', (c.blockers || []).join(' · '))); return art; }
    var t = c.test || {}, d = c.dryrun || {}, s = c.stack || {}, a = c.artifact || {};
    var ch = el('ul', 'ap-checks');
    ch.appendChild(chip(s.ok, false, s.ok ? 'From Hermes, signed' : 'Origin not proven'));
    ch.appendChild(chip(t.ok, t.state === 'waiting', t.ok ? 'Sandbox test passed' : t.state === 'waiting' ? 'Sandbox test running' : 'Sandbox test failed'));
    ch.appendChild(chip(d.ok, d.state === 'waiting', d.ok ? 'Dry run passed · 0 real model calls' : d.state === 'waiting' ? 'Dry run pending' : 'Dry run failed'));
    ch.appendChild(chip(a.on_disk_matches, !a.sha256, a.on_disk_matches ? 'Built file matches the tested one' : 'Built file not verified'));
    art.appendChild(ch);
    var fd = el('div', 'ap-fp'); fd.appendChild(el('p', 'ap-k', 'Build fingerprint (sha256)'));
    fd.appendChild(a.sha256 ? hexBlock(a.sha256) : el('p', 'ap-meta', 'No build yet'));
    fd.appendChild(el('p', 'ap-meta', 'Base commit ' + (c.base_commit || '?').slice(0, 12) + (a.bytes ? ' · ' + (a.bytes / 1048576).toFixed(1) + ' MB' : '') + (t.when ? ' · tested ' + t.when : '')));
    art.appendChild(fd);
    var m1 = more('The change · ' + (c.files || []).length + ' file' + ((c.files || []).length === 1 ? '' : 's'), false);
    m1.appendChild(diffView(c.diff, c.diff_truncated)); art.appendChild(m1);
    var m2 = more('Sandbox test · ' + (t.stages || []).length + ' steps', false);
    m2.appendChild(el('p', 'ap-why', t.why || '')); m2.appendChild(rows(t.stages, function (x) { return x.name + (x.ok ? ' — ok' : ' — exit ' + x.exit); })); art.appendChild(m2);
    var m3 = more('Dry run' + (d.port ? ' · port ' + d.port : ''), false);
    m3.appendChild(el('p', 'ap-why', (d.why || '') + (d.when ? ' (' + d.when + ')' : ''))); m3.appendChild(rows(d.checks, function (x) { return x.name + (x.detail ? ' — ' + x.detail : ''); })); art.appendChild(m3);
    if ((c.blockers || []).length && c.state !== 'approved') art.appendChild(el('p', 'ap-out t-bad', 'Cannot sign yet: ' + c.blockers.join(' · ')));
    var sign = el('div', 'ap-sign');
    if (c.state === 'approved') sign.appendChild(el('p', 'ap-out t-ok', 'You approved this build. Your signature is valid until ' + c.approval.when + '. The root deploy step installs it; this screen never deploys.'));
    if (c.state === 'rejected') sign.appendChild(el('p', 'ap-out', 'You rejected this build' + (c.approval && c.approval.when ? ' on ' + c.approval.when : '') + (c.approval && c.approval.note ? ': ' + c.approval.note : '.')));
    if (c.ready) {
      var tm = el('div', 'ap-timer'); tm.appendChild(el('span', null, 'One-time code'));
      var clk = el('span', 'ap-clock', '–:––'); tm.appendChild(clk); tm.appendChild(el('span', 'ap-meta ap-exp', ''));
      sign.appendChild(tm);
      var acts = el('div', 'ap-actions');
      var kv = keyst && keyst.active;
      var yes = el('button', 'hm-btn hm-primary ap-wide', kv && kv.scheme === 'ed25519' ? 'Approve with my key file' : 'Approve with my passkey'); yes.type = 'button';
      var no = el('button', 'hm-btn ap-wide ap-reject', 'Reject'); no.type = 'button';
      var renew = el('button', 'hm-btn ap-wide ap-ghost', 'Get a new code'); renew.type = 'button'; renew.hidden = true;
      acts.appendChild(yes); acts.appendChild(no); acts.appendChild(renew); sign.appendChild(acts);
      var out = el('p', 'ap-out'); sign.appendChild(out);
      var get = function () {
        out.textContent = ''; renew.hidden = true; yes.disabled = false;
        return api('/api/approvals/challenge', { id: c.id }).then(function (x) {
          chals[c.id] = x; tm.dataset.exp = x.expires; tm.querySelector('.ap-exp').textContent = 'valid until ' + x.when; tick();
        }).catch(function (e) { out.textContent = e.message; out.className = 'ap-out t-bad'; });
      };
      renew.onclick = get;
      yes.onclick = function () {
        var x = chals[c.id]; if (!x) return get();
        if (x.expires <= nowS()) { out.textContent = 'This code expired. Get a new one.'; return; }
        yes.disabled = true; out.className = 'ap-out'; out.textContent = 'Waiting for your device…';
        signWith({ scheme: x.scheme, key_id: x.key_id, passkey: x.passkey, fingerprint: kv && kv.fingerprint, short: kv && kv.short }, x.domain, x.body, x.webauthn_challenge)
          .then(function (asr) { return api('/api/approvals/approve', { nonce: x.nonce, assertion: asr }); })
          .then(function (r) { out.className = 'ap-out t-ok'; out.textContent = r.next; delete chals[c.id]; setTimeout(load, 900); })
          .catch(function (e) { yes.disabled = false; out.className = 'ap-out t-bad'; out.textContent = 'Not approved: ' + e.message; });
      };
      no.onclick = function () {
        var why = window.prompt('Reject this build? Optional note for Hermes:', '');
        if (why === null) return;
        api('/api/approvals/reject', { id: c.id, note: why }).then(function () { delete chals[c.id]; load(); })
          .catch(function (e) { out.className = 'ap-out t-bad'; out.textContent = e.message; });
      };
      get();
    }
    art.appendChild(sign);
    return art;
  }

  function tick() {
    document.querySelectorAll('.ap-timer[data-exp]').forEach(function (tm) {
      var left = +tm.dataset.exp - nowS(), clk = tm.querySelector('.ap-clock'), a = tm.closest('.ap-card');
      clk.textContent = left > 0 ? mmss(left) : 'expired';
      tm.className = 'ap-timer' + (left <= 0 ? ' gone' : left < 300 ? ' low' : '');
      if (a) { var bs = a.querySelectorAll('.ap-actions button'); if (bs.length === 3) { bs[0].disabled = left <= 0; bs[2].hidden = left > 0; } }
    });
  }

  function renderList(j) {
    var box = $('ap-list'); box.textContent = '';
    var items = j.items || [], ready = items.filter(function (i) { return i.state === 'ready'; }).length;
    $('st-wait').className = 'hm-dot ' + (ready ? 'warn' : 'ok'); $('st-wait-v').textContent = ready ? String(ready) : 'nothing';
    if (!items.length) { box.appendChild(el('p', 'hm-empty', 'No self-change requests. Hermes has not asked for anything.')); return; }
    items.forEach(function (c) { box.appendChild(card(c)); });
  }

  // ---- admin key ----
  function renderKey(k) {
    keyst = k;
    var st = { none: ['bad', 'not set'], pending: ['warn', 'waiting for you'], staged: ['warn', 'waiting for root'], active: ['ok', 'active'] }[k.state] || ['bad', k.state];
    $('st-key').className = 'hm-dot ' + st[0]; $('st-key-v').textContent = st[1];
    var a = k.active && k.active.fingerprint ? k.active : null;
    $('ap-active').hidden = !a;
    if (a) { var p = $('ap-active-fp'); p.replaceWith(hexBlock(a.fingerprint)); $('ap-active').lastChild.id = 'ap-active-fp'; }
    var txt = {
      none: 'No admin key yet. Create one on this device; only its public half goes to your Galion. Nothing can be approved until it is active.',
      pending: 'A new key is waiting. Compare the two fingerprints below, then activate.',
      staged: 'The new key is signed and staged. Root must install it before it counts.',
      active: 'Your admin key is active (' + (a && a.scheme === 'ed25519' ? 'key file' : 'passkey') + '). To replace it, create a new key here; you will sign with the current key too.'
    }[k.state] || '';
    $('ap-keystate').textContent = txt;
    $('ap-key').open = k.state !== 'active';
    $('ap-create').hidden = k.state === 'staged';
    $('ap-staged').hidden = k.state !== 'staged';
    if (k.state === 'staged') { var sp = $('ap-staged-fp'); sp.replaceWith(hexBlock(k.staged.fingerprint)); $('ap-staged').children[1].id = 'ap-staged-fp'; $('ap-cmd').textContent = k.install_command; }
    if (k.state === 'pending' && k.pending) {
      var dev = setup && setup.fp || localStorage.getItem('galion-owner-pending-fp') || '';
      showCompare(dev, k.pending.fingerprint, !!a && a.fingerprint !== k.pending.fingerprint);
    } else $('ap-compare').hidden = true;
  }
  function showCompare(dev, srv, replacing) {
    $('ap-compare').hidden = false;
    $('ap-fp-dev').textContent = dev ? group4(dev) : '(made on another device; compare there)';
    $('ap-fp-srv').textContent = group4(srv);
    var m = $('ap-match');
    if (dev) { m.textContent = dev === srv ? 'They match.' : 'They do NOT match. Do not activate.'; m.className = 'ap-match ' + (dev === srv ? 'ok' : 'bad'); }
    else { m.textContent = ''; m.className = 'ap-match'; }
    $('ap-replace-note').hidden = !replacing;
    $('ap-confirm').checked = false; $('ap-activate').disabled = true;
    $('ap-confirm').disabled = !!dev && dev !== srv;
  }
  function registered(r, devFp) {
    setup = { fp: devFp }; localStorage.setItem('galion-owner-pending-fp', devFp);
    note('ap-key-note', r.next); return loadKey();
  }
  function newPasskey() {
    note('ap-key-note', 'Asking your device for a new passkey…');
    var b;
    api('/api/owner-key/begin', { scheme: 'webauthn-es256' }).then(function (x) {
      b = x;
      return navigator.credentials.create({ publicKey: {
        challenge: unb64u(x.webauthn_challenge), rp: { id: x.rp_id, name: 'Galion One' },
        user: { id: unb64u(x.user_id), name: 'slavik', displayName: 'Galion owner (Slavik)' },
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
        authenticatorSelection: { residentKey: 'preferred', userVerification: 'required' }, attestation: 'none', timeout: 120000 } });
    }).then(function (c) {
      if (!c || !c.response.getPublicKey) throw new Error('This browser cannot hand out the passkey public key. Use a key file instead.');
      if (c.response.getPublicKeyAlgorithm && c.response.getPublicKeyAlgorithm() !== -7) throw new Error('The passkey is not P-256 (ES256).');
      var spki = c.response.getPublicKey();
      return sha(new Uint8Array(spki).slice(-65)).then(function (fp) {
        return api('/api/owner-key/register', { nonce: b.nonce, public_key_spki_hex: hex(spki), credential_id: b64u(c.rawId), client_data_json: b64u(c.response.clientDataJSON) })
          .then(function (r) { return registered(r, fp); });
      });
    }).catch(function (e) { note('ap-key-note', 'No key made: ' + e.message, true); });
  }
  function storeEd(kp, pubRaw) {
    return sha(pubRaw).then(function (fp) {
      return crypto.subtle.exportKey('pkcs8', kp.privateKey).then(function (p8) {
        return crypto.subtle.importKey('pkcs8', p8, { name: 'Ed25519' }, false, ['sign']).then(function (priv) {
          return keyPut({ fp: fp, scheme: 'ed25519', pub: hex(pubRaw), priv: priv, made: Date.now() }).then(function () { return { fp: fp, p8: p8, priv: priv }; });
        });
      });
    });
  }
  function regEd(k) {
    return api('/api/owner-key/begin', { scheme: 'ed25519' }).then(function (b) {
      return crypto.subtle.sign({ name: 'Ed25519' }, k.priv, te.encode(b.domain + '\n' + b.body)).then(function (sig) {
        return keyGet(k.fp).then(function (rec) { return api('/api/owner-key/register', { nonce: b.nonce, public_key_hex: rec.pub, sig: hex(sig) }); });
      });
    }).then(function (r) { return registered(r, k.fp); });
  }
  function newKeyFile() {
    var kp;
    crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']).then(function (x) { kp = x; return crypto.subtle.exportKey('raw', kp.publicKey); })
      .then(function (raw) { return storeEd(kp, raw); })
      .then(function (k) {
        var a = document.createElement('a'); a.href = URL.createObjectURL(new Blob([k.p8], { type: 'application/octet-stream' }));
        a.download = 'galion-owner-key-' + k.fp.slice(0, 16) + '.pk8'; document.body.appendChild(a); a.click(); a.remove();
        note('ap-key-note', 'Backup key file saved on this device. Keep it offline; it is never uploaded.');
        return regEd(k);
      }).catch(function (e) { note('ap-key-note', 'No key made: ' + (e.message || 'this browser has no Ed25519; use a passkey'), true); });
  }
  function importKeyFile(f) {
    f.arrayBuffer().then(function (p8) { return crypto.subtle.importKey('pkcs8', p8, { name: 'Ed25519' }, true, ['sign']); })
      .then(function (priv) { return crypto.subtle.exportKey('jwk', priv).then(function (j) { return storeEd({ privateKey: priv }, unb64u(j.x)); }); })
      .then(function (k) {
        if (keyst && keyst.active && keyst.active.fingerprint === k.fp) { note('ap-key-note', 'Key file loaded. It is your active key; you can approve on this device now.'); return load(); }
        return regEd(k);
      }).catch(function (e) { note('ap-key-note', 'Could not read that key file: ' + e.message, true); });
  }
  function activate() {
    $('ap-activate').disabled = true; note('ap-key-note', 'Sign with the new key' + (keyst.active ? ', then the current one' : '') + '…');
    api('/api/owner-key/activate-challenge', {}).then(function (x) {
      return signWith(x.activate.key, x.activate.domain, x.activate.body, x.activate.webauthn_challenge).then(function (na) {
        if (!x.change) return [na, null];
        return signWith(x.change.key, x.change.domain, x.change.body, x.change.webauthn_challenge).then(function (ca) { return [na, ca]; });
      }).then(function (s) { return api('/api/owner-key/activate', { nonce: x.nonce, confirmed: true, activate: s[0], change: s[1] }); });
    }).then(function (r) { localStorage.removeItem('galion-owner-pending-fp'); setup = null; note('ap-key-note', r.next); return load(); })
      .catch(function (e) { $('ap-activate').disabled = !$('ap-confirm').checked; note('ap-key-note', 'Not activated: ' + e.message, true); });
  }

  function loadKey() { return api('/api/owner-key').then(renderKey); }
  function load() {
    return api('/api/approvals').then(function (j) {
      list = j; skew = j.now - Date.now() / 1000; renderKey(j.owner_key); renderList(j);
    }).catch(function (e) { note('ap-note', e.message, true); $('ap-list').textContent = ''; });
  }

  $('ap-new-passkey').onclick = newPasskey;
  $('ap-new-file').onclick = newKeyFile;
  $('ap-import').onclick = function () { $('ap-import-file').click(); };
  $('ap-import-file').onchange = function (e) { if (e.target.files[0]) importKeyFile(e.target.files[0]); e.target.value = ''; };
  $('ap-confirm').onchange = function () { $('ap-activate').disabled = !this.checked; };
  $('ap-activate').onclick = activate;
  if (!window.PublicKeyCredential) { $('ap-new-passkey').disabled = true; }
  setInterval(tick, 1000);
  load();
})();
