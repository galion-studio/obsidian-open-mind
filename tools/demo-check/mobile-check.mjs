// SPDX-License-Identifier: MIT
// Headless Chrome phone check of the Hermes page (390x844, Android Chrome UA) over CDP.
// Usage: node mobile-check.mjs <base-url> <out-png> [--send]
//   --send  also sends one River chat ("Reply with the single word OK.") through the UI.
// Prints PASS/FAIL lines; exit code = failures. No keys are touched.
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
const [base, png, flag] = process.argv.slice(2);
const send = process.argv.includes('--send');
const port = 9300 + Math.floor(Math.random() * 500);
const dir = mkdtempSync('/tmp/mchk-');
const chrome = spawn('google-chrome', ['--headless=new', '--no-sandbox', '--disable-gpu', `--remote-debugging-port=${port}`, `--user-data-dir=${dir}`, 'about:blank'], { stdio: 'ignore' });
const sleep = ms => new Promise(r => setTimeout(r, ms));
let fails = 0; const res = (ok, n, d = '') => { if (!ok) fails++; console.log(`${ok ? 'PASS' : 'FAIL'}  ${n.padEnd(34)} ${d}`); };
let ver; for (let i = 0; i < 50 && !ver; i++) { try { ver = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); } catch { await sleep(200); } }
const tgt = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
const ws = new WebSocket(tgt.webSocketDebuggerUrl); await new Promise(r => ws.onopen = r);
let id = 0; const pend = new Map(); const errors = []; const bad = [];
ws.onmessage = e => { const m = JSON.parse(e.data);
  if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); return; }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') errors.push(m.params.args.map(a => a.value ?? a.description).join(' '));
  if (m.method === 'Network.responseReceived' && m.params.response.status >= 400) bad.push(`${m.params.response.status} ${m.params.response.url.replace(base, '')}`);
};
const cdp = (method, params = {}) => new Promise(r => { const i = ++id; pend.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const ev = async (expr) => { const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }); return r.result?.result?.value; };
for (const d of ['Page', 'Runtime', 'Network']) await cdp(`${d}.enable`);
await cdp('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 3, mobile: true });
await cdp('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
await cdp('Network.setUserAgentOverride', { userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36' });
await cdp('Page.navigate', { url: base + '/hermes' }); await sleep(4000);
const s = await ev(`(() => { const q = document.getElementById('hm-q'), p = document.getElementById('hm-pick'), snd = document.getElementById('hm-send');
  const mic = document.querySelector('.hm-mic-slot button, .hm-mic-slot [role=button]');
  const small = [...document.querySelectorAll('button,[role=button],label.hm-opt,a.hm-item')].filter(b => { const r = b.getBoundingClientRect(); return r.width > 0 && r.height > 0 && (r.height < 40 || r.width < 40); }).map(b => (b.id || b.className || b.tagName) + ':' + Math.round(b.getBoundingClientRect().width) + 'x' + Math.round(b.getBoundingClientRect().height));
  return { sw: document.documentElement.scrollWidth, iw: innerWidth, q: !!q && !q.disabled, pick: !!p && !p.disabled, send: !!snd, mic: !!mic, micText: mic ? (mic.getAttribute('aria-label') || mic.textContent).trim() : '', prov: [...document.querySelectorAll('input[name=hm-provider]')].map(x => x.value + (x.checked ? '*' : '')), small: small.slice(0, 8), title: document.title }; })()`);
res(s && s.sw <= s.iw, 'no horizontal scroll at 390px', s && `scrollWidth ${s.sw} / ${s.iw}`);
res(s?.q, 'composer enabled'); res(s?.pick, 'model button enabled'); res(s?.send, 'send button present');
res(s?.mic, 'dictation mic button present', s?.micText || '');
res((s?.prov || []).length >= 2, 'providers shown', (s?.prov || []).join(' '));
console.log(`INFO  small tap targets (<40px): ${(s?.small || []).join(' ') || 'none'}`);
await ev(`document.getElementById('hm-pick').click()`); await sleep(1500);
for (const p of ['river', 'openrouter']) {
  await ev(`document.querySelector('.hm-tab[data-p=${p}]').click()`); await sleep(2500);
  const n = await ev(`(() => { const sh = document.getElementById('hm-sheet'); return { open: sh && !sh.hidden && getComputedStyle(sh).display !== 'none', rows: sh ? [...sh.querySelectorAll('li')].filter(l => !l.hidden).length : 0, stars: sh ? sh.querySelectorAll('[data-star]').length : 0, search: !!sh?.querySelector('input[type=search],.hm-search input,input.hm-search') }; })()`);
  res(n?.open && n.rows > 0, `model sheet ${p} lists models`, n ? `${n.rows} rows, ${n.stars} star buttons, search=${n.search}` : '');
}
const shot = await cdp('Page.captureScreenshot', { format: 'png' }); writeFileSync(png.replace(/\.png$/, '-sheet.png'), Buffer.from(shot.result.data, 'base64'));
await ev(`document.getElementById('hm-sheet-x').click()`); await sleep(500);
if (send) {
  await ev(`(() => { const r = document.querySelector('input[name=hm-provider][value=river]'); if (r) { r.click(); r.dispatchEvent(new Event('change', {bubbles:true})); } })()`); await sleep(800);
  await ev(`(() => { const q = document.getElementById('hm-q'); q.value = 'Reply with the single word OK.'; q.dispatchEvent(new Event('input', {bubbles:true})); document.getElementById('hm-send').click(); })()`);
  let txt = '', t0 = Date.now(); let sawStop = false;
  while (Date.now() - t0 < 90000) { await sleep(1500); const st = await ev(`({ stop: !document.getElementById('hm-stop').hidden, log: (document.querySelector('.hm-log')?.innerText || '').slice(-400), meta: document.querySelector('.hm-answer-meta')?.innerText || '' })`);
    if (st.stop) sawStop = true; txt = st.log + ' | ' + st.meta; if (!st.stop && /Answered by/i.test(st.meta)) break; }
  res(/\bOK\b/.test(txt) && /Answered by/i.test(txt), 'UI chat round trip (River)', txt.replace(/\s+/g, ' ').slice(-160));
  console.log(`INFO  stop button seen while streaming: ${sawStop} (a fast answer can finish between polls)`);
}
if (process.argv.includes('--fail')) {
  // A provider that is down (laptop offline) must give a readable error, not a hang. Then back to River.
  await ev(`(() => { const r = document.querySelector('input[name=hm-provider][value=laptop]'); if (r) { r.click(); r.dispatchEvent(new Event('change', {bubbles:true})); } })()`); await sleep(800);
  const dis = await ev(`document.getElementById('hm-send').disabled`);
  let msg = '';
  if (!dis) { await ev(`(() => { const q = document.getElementById('hm-q'); q.value = 'ping'; q.dispatchEvent(new Event('input', {bubbles:true})); document.getElementById('hm-send').click(); })()`); await sleep(4000); }
  msg = await ev(`(document.getElementById('hm-err')?.innerText || '') + ' ' + (document.getElementById('hm-note')?.innerText || '') + ' ' + (document.querySelector('.hm-status')?.innerText || '')`);
  res(dis || /offline|not set up|failed|error/i.test(msg), 'provider down -> readable error', (dis ? 'send disabled; ' : '') + msg.replace(/\s+/g, ' ').trim().slice(0, 160));
  const shot3 = await cdp('Page.captureScreenshot', { format: 'png' }); writeFileSync(png.replace(/\.png$/, '-fail.png'), Buffer.from(shot3.result.data, 'base64'));
  await ev(`(() => { const r = document.querySelector('input[name=hm-provider][value=river]'); if (r) { r.click(); r.dispatchEvent(new Event('change', {bubbles:true})); } })()`); await sleep(1500);
}
const shot2 = await cdp('Page.captureScreenshot', { format: 'png' }); writeFileSync(png, Buffer.from(shot2.result.data, 'base64'));
res(errors.length === 0, 'no JS errors', errors.slice(0, 3).join(' | '));
res(bad.filter(b => !/favicon/.test(b)).length === 0, 'no HTTP >= 400 responses', bad.slice(0, 5).join(' | '));
ws.close(); chrome.kill('SIGKILL'); process.exit(fails);
