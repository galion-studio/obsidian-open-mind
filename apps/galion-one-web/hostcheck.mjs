// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// Phone-width Chromium check per host: screenshots + root background from the first frame (flash check).
// LOCAL=<port> routes https://{one.,www.,}galion.app/ to a local stand-in server.
// usage: node hostcheck.mjs <outprefix> <url> [waits] [resolverRules]
import { chromium } from 'playwright-core';
const [out, url, waits='60,150,400,1300,2700,5000', rules=''] = process.argv.slice(2);
const args = ['--no-sandbox']; if (rules) args.push('--host-resolver-rules=' + rules);
const b = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args });
const p = await b.newPage({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
const local = process.env.LOCAL;
if (local) await p.route(/^https:\/\/(one\.|www\.)?galion\.app\//, async (route) => { const u = new URL(route.request().url()); const r = await fetch("http://127.0.0.1:" + local + u.pathname + u.search, { method: route.request().method() }); route.fulfill({ status: r.status, headers: { "content-type": r.headers.get("content-type") || "" }, body: Buffer.from(await r.arrayBuffer()) }); });
const errs=[]; p.on('pageerror', e=>errs.push(String(e)));
// Sample the root background colour as early as possible (first paint), via a rAF loop started before any page script.
await p.addInitScript(() => { window.__bg = []; const f = () => { try { const h = document.documentElement; window.__bg.push([Math.round(performance.now()), h.className, getComputedStyle(h).backgroundColor, document.body ? getComputedStyle(document.body).backgroundColor : '-']); } catch (e) {} if (window.__bg.length < 400) requestAnimationFrame(f); }; requestAnimationFrame(f); });
const t0 = Date.now(); await p.goto(url, { waitUntil: 'commit' });
for (const w of waits.split(',').map(Number)) { const d = w - (Date.now() - t0); if (d > 0) await p.waitForTimeout(d); await p.screenshot({ path: `${out}-${w}ms.png` }); }
const bg = await p.evaluate(() => window.__bg);
const uniq = [...new Set(bg.map(x => x.slice(1).join(' | ')))];
const meta = await p.evaluate(() => document.querySelector('meta[name="theme-color"]').content);
console.log(JSON.stringify({ url, errs, firstFrame: bg[0], distinctRootStates: uniq, theme: meta }));
await b.close();
