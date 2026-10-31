// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Console phone home: fills the status tiles from GET /api/v1/phone/status
// (read-only). textContent only; times in Europe/Zagreb.
(function () {
  'use strict';
  var $ = function (i) { return document.getElementById(i); };
  function txt(id, t) { var e = $(id); if (e) e.textContent = t == null ? '' : String(t); }
  function dot(id, c) { var e = $(id); if (e) e.className = 'hm-dot' + (c ? ' ' + c : ''); }
  var zf = null;
  try {
    zf = new Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/Zagreb', hour: '2-digit', minute: '2-digit', timeZoneName: 'short' });
  } catch (e) { zf = null; }
  function clock(sec) {
    var d = new Date(sec * 1000);
    return zf ? zf.format(d) : d.toTimeString().slice(0, 5);
  }
  function usd(n) { return typeof n === 'number' ? '$' + n.toFixed(2) : '?'; }
  function ago(s) {
    if (typeof s !== 'number' || s < 0) return '';
    if (s < 90) return Math.round(s) + ' s ago';
    if (s < 5400) return Math.round(s / 60) + ' min ago';
    if (s < 172800) return Math.round(s / 3600) + ' h ago';
    return Math.round(s / 86400) + ' d ago';
  }
  function plural(n, word) { return n + ' ' + word + (n === 1 ? '' : 's'); }
  function li(parent, cls, parts) {
    var l = document.createElement('li');
    if (cls) l.className = cls;
    parts.forEach(function (p) { l.appendChild(p); });
    parent.appendChild(l);
  }
  function el(tag, cls, t) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (t != null) e.textContent = String(t);
    return e;
  }
  function hermes(h) {
    if (!h) return;
    dot('ph-hermes-dot', h.up ? (h.nerve && h.nerve !== 'healthy' ? 'warn' : 'ok') : 'bad');
    txt('ph-hermes-v', h.up ? 'Up' : 'Down');
    var s = [];
    s.push(h.build ? 'build ' + h.build : 'build unknown');
    if (h.nerve) s.push('nerve: ' + h.nerve);
    if (!h.up && h.error) s.push(h.error);
    txt('ph-hermes-s', s.join(' · '));
  }
  function spend(p) {
    var bar = $('ph-spend-bar');
    if (!p || !p.ok) {
      dot('ph-spend-dot', 'warn');
      txt('ph-spend-v', 'No snapshot');
      txt('ph-spend-s', (p && p.error) || 'The spend snapshot is not readable.');
      if (bar) bar.className = 'ph-bar warn';
      return;
    }
    var lvl = p.level === 'ok' ? '' : (p.level === 'near' ? 'warn' : 'bad');
    if (p.stale) lvl = lvl || 'warn';
    dot('ph-spend-dot', lvl || 'ok');
    txt('ph-spend-v', usd(p.fresh_usd) + ' of ' + usd(p.limit_usd));
    var f = $('ph-spend-fill');
    if (f && p.limit_usd > 0) f.style.width = Math.min(100, Math.max(0, 100 * p.fresh_usd / p.limit_usd)).toFixed(1) + '%';
    var st = $('ph-spend-stop');
    if (st && p.limit_usd > 0) st.style.left = Math.min(100, 100 * p.stop_line_usd / p.limit_usd).toFixed(1) + '%';
    if (bar) bar.className = 'ph-bar' + (lvl ? ' ' + lvl : '');
    var s = ['stop line ' + usd(p.stop_line_usd), usd(p.left_to_stop_line_usd) + ' left before it'];
    if (typeof p.day_usd === 'number') s.push('today ' + usd(p.day_usd));
    s.push('baseline $' + (typeof p.baseline_usd === 'number' ? p.baseline_usd.toFixed(3) : '?'));
    s.push(p.stale ? 'snapshot OLD, ' + ago(p.age_s) : 'snapshot ' + ago(p.age_s));
    txt('ph-spend-s', s.join(' · '));
  }
  function watchdog(w) {
    var list = $('ph-wd-list');
    if (list) list.textContent = '';
    if (!w || !w.ok) {
      dot('ph-wd-dot', 'bad');
      txt('ph-wd-v', 'Unreachable');
      txt('ph-wd-s', (w && w.error) || '');
      if (list) li(list, 'ph-s', [el('span', '', 'The watchdog did not answer.')]);
      return;
    }
    dot('ph-wd-dot', w.awaiting > 0 ? 'warn' : 'ok');
    txt('ph-wd-v', w.awaiting + ' waiting');
    var s = [plural(w.escalations_open, 'escalation') + ' open'];
    if (typeof w.actions_pending === 'number') s.push(plural(w.actions_pending, 'action') + ' to approve');
    if (w.mode) s.push(w.mode);
    txt('ph-wd-s', s.join(' · '));
    if (!list) return;
    if (!w.items || !w.items.length) {
      li(list, 'ph-s', [el('span', '', 'Nothing waiting.')]);
      return;
    }
    w.items.forEach(function (it) {
      var meta = [it.kind];
      if (it.severity) meta.push(it.severity);
      if (it.unit) meta.push(it.unit);
      if (typeof it.ts === 'number') meta.push(clock(it.ts));
      li(list, '', [document.createTextNode(it.what || it.id), el('span', '', meta.join(' · '))]);
    });
    if (w.awaiting > w.items.length) li(list, 'ph-s', [el('span', '', '+' + (w.awaiting - w.items.length) + ' more in the full console')]);
  }
  function health(rows, b) {
    var ul = $('ph-health');
    if (ul && rows) {
      ul.textContent = '';
      rows.forEach(function (r) {
        li(ul, '', [el('span', 'hm-dot ' + (r.ok ? 'ok' : (r.status === 'down' ? 'bad' : 'warn'))), document.createTextNode(r.name + ' '), el('b', '', r.status), el('em', '', r.detail || '')]);
      });
    }
    if (b && b.ok) {
      var s = 'Backups: ' + b.state;
      if (typeof b.newest_age_h === 'number') s += ' · newest ' + b.newest_age_h.toFixed(1) + ' h old';
      if (typeof b.archives === 'number') s += ' · ' + b.archives + ' archives';
      if (b.problems && b.problems.length) s += ' · ' + b.problems.join('; ');
      if (b.stale) s += ' · snapshot OLD';
      txt('ph-backup', s);
    } else {
      txt('ph-backup', 'Backups: ' + ((b && b.error) || 'no snapshot'));
    }
  }
  function render(d) {
    hermes(d.hermes);
    spend(d.spend);
    watchdog(d.watchdog);
    health(d.health, d.backup);
    txt('ph-upd', 'Updated ' + clock(d.now) + ' · refreshes every 30 s');
  }
  function unpaired() {
    ['ph-hermes-v', 'ph-wd-v', 'ph-spend-v'].forEach(function (i) { txt(i, 'locked'); });
    ['ph-wd-list', 'ph-health'].forEach(function (i) {
      var e = $(i);
      if (e) { e.textContent = ''; li(e, 'ph-s', [el('span', '', 'Pair this phone to see this.')]); }
    });
    var bar = $('ph-spend-bar');
    if (bar) bar.hidden = true;
    txt('ph-upd', 'This phone is not paired with the console yet.');
  }
  function load() {
    fetch('/api/v1/phone/status', { credentials: 'same-origin', cache: 'no-store' })
      .then(function (r) {
        if (r.status === 401) { unpaired(); return null; }
        return r.json();
      })
      .then(function (d) { if (d) render(d); })
      .catch(function () { txt('ph-upd', 'Could not reach the console. Retrying in 30 s.'); });
  }
  load();
  setInterval(function () { if (!document.hidden) load(); }, 30000);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) load(); });
})();
