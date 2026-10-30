// node app2x.mjs <base> <prefix> : extra Chromium states to match console shots 06-12.
import * as pw from 'playwright-core';
const [base, pre = ''] = process.argv.slice(2);
const D = '/workspace/swarm/demo/galion-one-v2';
const b = await pw.chromium.launch({ args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] });
async function room(vp, extra = {}) {
  const ctx = await b.newContext({ viewport: vp, deviceScaleFactor: vp.width < 600 ? 2 : 1, isMobile: vp.width < 600, hasTouch: vp.width < 600, permissions: ['microphone'], ...extra });
  const p = await ctx.newPage();
  await p.goto(base + '/'); await p.waitForTimeout(3600);
  if (await p.isVisible('#g1-code')) { await p.fill('#g1-code', process.env.CODE || 'FIXTURE42'); await p.click('#g1-pair button[type=submit]'); }
  await p.waitForSelector('#hm-form:not([hidden])'); await p.waitForTimeout(1200);
  return p;
}
const phone = { width: 390, height: 844 };
let p = await room(phone);
await p.route('**/admin/say', r => r.fulfill({ status: 200, contentType: 'application/json', body: '{"say":"Again"}' }));
await p.fill('#hm-q', 'Summarise my week'); await p.click('#hm-send'); await p.waitForTimeout(900);
await p.screenshot({ path: `${D}/app2-${pre}chromium-12-error.png` });
await p.click('#hm-model'); await p.waitForTimeout(300); await p.screenshot({ path: `${D}/app2-${pre}chromium-13-model.png` });
await p.click('#g1-sound'); await p.waitForTimeout(300); await p.screenshot({ path: `${D}/app2-${pre}chromium-14-sound.png` });
await p.click('#g1-sound');
await p.context().close();
p = await room({ width: 1280, height: 800 });
await p.screenshot({ path: `${D}/app2-${pre}chromium-15-desktop-idle.png` });
if (!pre) { await p.fill('#hm-q', 'What is the weather in Zagreb today?'); await p.click('#hm-send'); await p.waitForSelector('.hm-assistant'); await p.waitForTimeout(2500);
await p.screenshot({ path: `${D}/app2-${pre}chromium-16-desktop-answered.png` }); }
await p.context().close();
p = await room(phone, { reducedMotion: 'reduce' }); await p.screenshot({ path: `${D}/app2-${pre}chromium-17-reduced-motion.png` }); await p.context().close();
const c = await b.newContext({ viewport: phone, deviceScaleFactor: 2, javaScriptEnabled: false }); const q = await c.newPage();
await q.goto(base + '/'); await q.waitForTimeout(1500); await q.screenshot({ path: `${D}/app2-${pre}chromium-18-js-off.png` });
await b.close(); console.log('done');
