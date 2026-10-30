// node app2.mjs <base> <chromium|firefox> [prefix]  -> /workspace/swarm/demo/galion-one-v2/app2-<prefix><browser>-NN-*.png
import * as pw from 'playwright-core';
const [base, which, pre = ''] = process.argv.slice(2);
const D = '/workspace/swarm/demo/galion-one-v2';
const opts = which === 'chromium'
  ? { args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'] }
  : { firefoxUserPrefs: { 'media.navigator.streams.fake': true, 'media.navigator.permission.disabled': true, 'permissions.default.microphone': 1 } };
const b = await pw[which].launch(opts);
const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: which !== 'firefox', hasTouch: true, ...(which === 'chromium' ? { permissions: ['microphone'] } : {}) });
const p = await ctx.newPage();
const errs = []; p.on('pageerror', e => errs.push(String(e))); p.on('console', m => { if (m.type() === 'error') errs.push(m.text()); });
const shot = async n => { await p.screenshot({ path: `${D}/app2-${pre}${which}-${n}.png` }); console.log('shot', n); };
await p.goto(base + '/');
await p.waitForTimeout(700); await shot('01-loading');
await p.waitForTimeout(3300); await shot('02-pair');
await p.fill('#g1-code', process.env.CODE || 'FIXTURE42'); await p.click('#g1-pair button[type=submit]');
await p.waitForSelector('#hm-form:not([hidden])'); await p.waitForTimeout(1500); await shot('03-idle');
const st = await p.evaluate(() => ({ theme: document.querySelector('meta[name=theme-color]').content, mic: document.getElementById('hm-mic').dataset.state, stt: document.getElementById('st-stt-v').textContent, mind: document.getElementById('st-mind-v').textContent, model: document.getElementById('hm-model-v').textContent }));
console.log('state', JSON.stringify(st));
await p.click('#hm-mic'); await p.waitForTimeout(2600); await shot('04-listening');
console.log('mic live', await p.evaluate(() => document.getElementById('hm-mic').dataset.state + ' | ' + document.getElementById('hm-partial').textContent));
await p.click('#hm-mic'); await p.waitForTimeout(1200); await shot('05-dictated');
console.log('after stop', await p.evaluate(() => document.getElementById('hm-mic').dataset.state + ' | ' + document.getElementById('hm-q').value));
await p.fill('#hm-q', 'What is the weather in Zagreb today?'); await p.waitForTimeout(300); await shot('06-typing');
await p.click('#hm-send'); await p.waitForTimeout(500); await shot('07-thinking');
await p.waitForSelector('.hm-assistant'); await p.waitForTimeout(300); await shot('08-speaking');
await p.waitForTimeout(2500); await shot('09-answered');
console.log('answer', await p.evaluate(() => document.querySelector('.hm-assistant .hm-text').textContent.slice(0, 60)), 'state', await p.evaluate(() => document.body.dataset.state));
await p.click('#g1-dev summary'); await p.click('#g1-invite'); await p.waitForTimeout(600);
await p.evaluate(() => { document.getElementById('g1-invite-code').textContent = 'ABCD-EFGH'; var t = document.getElementById('g1-ntfy-topic'); if (t) t.textContent = 'ntfy.sh/your-topic'; }); // never screenshot a real code
await shot('10-devices');
await p.click('#g1-dev summary');
// Dictation offline: the health probe says down.
await p.route('**/stt/health', r => r.fulfill({ status: 200, contentType: 'application/json', body: '{"ok":false,"status":502,"error":"connect"}' }));
await p.reload(); await p.waitForTimeout(4200); await shot('11-mic-offline');
console.log('offline mic', await p.evaluate(() => document.getElementById('hm-mic').dataset.state + ' | ' + document.getElementById('st-stt-v').textContent));
console.log('errors', JSON.stringify(errs));
await b.close();
