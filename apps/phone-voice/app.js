// SPDX-License-Identifier: MIT
// Open Mind phone voice client v0.1. Plain JS, no framework, no build step.
// Wire protocol = persona-plex-relay (crates/persona-plex-relay/src/protocol.rs),
// i.e. the moshi/PersonaPlex binary kinds, first byte of every WS message:
//   0x00 handshake (relay/GPU ready)   0x01 audio: Opus in Ogg pages, 24 kHz mono
//   0x02 text  0x05 error  0x06 ping
//   0x40 speaker status [has_score, score_milli i16 BE, decision 0..3]
//   0x41 fallback [reason 0..3]   0x42 bye (UTF-8 reason)
// The client only ever sends 0x01. Push-to-talk gates the mic to silence; the
// stream keeps flowing because the model steps once per 80 ms input frame.
"use strict";
const RATE = 24000, FRAME = 1920; // 80 ms (Mimi frame)
const $ = (id) => document.getElementById(id);
const cfg = { relay_url: "", text_url: "", device_id: "", token: "" };
let ws = null, ctx = null, cap = null, player = null, enc = null, dec = null, ogg = null, demux = null;
let pttDown = false, ready = false, retry = 0, ts = 0;

const log = (s) => { const l = $("log"); l.textContent += new Date().toLocaleTimeString() + " " + s + "\n"; l.scrollTop = 1e9; };
const pill = (id, text, cls) => { const e = $(id); e.textContent = text; e.className = "pill " + (cls || ""); };

async function loadConfig() {
  try { Object.assign(cfg, await (await fetch("config.json", { cache: "no-store" })).json()); } catch (_) {}
  try { Object.assign(cfg, JSON.parse(localStorage.getItem("omvoice.cfg") || "{}")); } catch (_) {}
  $("relay").value = cfg.relay_url; $("texturl").value = cfg.text_url; $("dev").value = cfg.device_id;
}
$("save").onclick = () => {
  cfg.relay_url = $("relay").value.trim(); cfg.text_url = $("texturl").value.trim(); cfg.device_id = $("dev").value.trim();
  localStorage.setItem("omvoice.cfg", JSON.stringify(cfg)); log("settings saved on this device");
};

// ---- Ogg (RFC 3533) framing for Opus (RFC 7845) ----
const CRC = new Uint32Array(256).map((_, i) => { let r = i << 24; for (let k = 0; k < 8; k++) r = r & 0x80000000 ? (r << 1) ^ 0x04c11db7 : r << 1; return r >>> 0; });
function oggPage(packet, granule, serial, seq, flags) {
  const segs = []; let n = packet.length; for (; n >= 255; n -= 255) segs.push(255); segs.push(n);
  const p = new Uint8Array(27 + segs.length + packet.length), v = new DataView(p.buffer);
  p.set([79, 103, 103, 83]); p[5] = flags;
  v.setUint32(6, granule % 4294967296, true); v.setUint32(10, Math.floor(granule / 4294967296), true);
  v.setUint32(14, serial, true); v.setUint32(18, seq, true); p[26] = segs.length; p.set(segs, 27); p.set(packet, 27 + segs.length);
  let c = 0; for (const b of p) c = ((c << 8) ^ CRC[((c >>> 24) ^ b) & 255]) >>> 0; v.setUint32(22, c, true);
  return p;
}
function oggWriter() {
  const serial = (Math.random() * 4294967295) >>> 0; let seq = 0, gran = 0;
  const head = new Uint8Array(19), hv = new DataView(head.buffer);
  head.set(new TextEncoder().encode("OpusHead")); head[8] = 1; head[9] = 1; hv.setUint16(10, 312, true); hv.setUint32(12, RATE, true);
  const vendor = new TextEncoder().encode("om-phone-voice"), tags = new Uint8Array(16 + vendor.length), tv = new DataView(tags.buffer);
  tags.set(new TextEncoder().encode("OpusTags")); tv.setUint32(8, vendor.length, true); tags.set(vendor, 12);
  return {
    headers: () => [oggPage(head, 0, serial, seq++, 2), oggPage(tags, 0, serial, seq++, 0)],
    packet: (pkt, samples48k) => oggPage(pkt, (gran += samples48k), serial, seq++, 0),
  };
}
function oggReader(onPacket) {
  let buf = new Uint8Array(0), pend = [];
  return (chunk) => {
    const b = new Uint8Array(buf.length + chunk.length); b.set(buf); b.set(chunk, buf.length); buf = b;
    for (;;) {
      if (buf.length < 27) return;
      if (buf[0] !== 79 || buf[1] !== 103 || buf[2] !== 103 || buf[3] !== 83) { buf = new Uint8Array(0); pend = []; return; }
      const ns = buf[26], hl = 27 + ns; if (buf.length < hl) return;
      let body = 0; for (let i = 0; i < ns; i++) body += buf[27 + i]; if (buf.length < hl + body) return;
      let off = hl;
      for (let i = 0; i < ns; i++) {
        const l = buf[27 + i]; pend.push(buf.slice(off, off + l)); off += l;
        if (l < 255) { const n = pend.reduce((a, x) => a + x.length, 0), pk = new Uint8Array(n); let o = 0; for (const x of pend) { pk.set(x, o); o += x.length; } pend = []; onPacket(pk); }
      }
      buf = buf.slice(hl + body);
    }
  };
}
const isHeader = (p) => p.length >= 8 && p[0] === 79 && p[1] === 112 && p[2] === 117 && p[3] === 115; // "Opus"

// ---- AudioWorklets: capture (resample to 24 kHz, 80 ms float frames) and playback ----
const WORKLET = `
class Cap extends AudioWorkletProcessor{constructor(){super();this.r=sampleRate/${RATE};this.p=0;this.b=new Float32Array(${FRAME});this.n=0}
process(i){const x=i[0]&&i[0][0];if(!x)return true;let p=this.p;while(p<x.length){const k=p|0,f=p-k,a=x[k],c=k+1<x.length?x[k+1]:a;
this.b[this.n++]=a+(c-a)*f;if(this.n===${FRAME}){this.port.postMessage(this.b.buffer,[this.b.buffer]);this.b=new Float32Array(${FRAME});this.n=0}p+=this.r}
this.p=p-x.length;return true}}
class Play extends AudioWorkletProcessor{constructor(){super();this.q=new Float32Array(sampleRate*8);this.w=0;this.rd=0;
this.port.onmessage=(e)=>{if(e.data==="flush"){this.rd=this.w;return}const s=new Float32Array(e.data.pcm),r=e.data.rate/sampleRate,n=Math.floor(s.length/r);
for(let j=0;j<n;j++){const t=j*r,k=t|0,f=t-k,a=s[k],c=k+1<s.length?s[k+1]:a;this.q[this.w++%this.q.length]=a+(c-a)*f}}}
process(_,o){const y=o[0][0];for(let j=0;j<y.length;j++)y[j]=this.rd<this.w?this.q[this.rd++%this.q.length]:0;return true}}
registerProcessor("om-cap",Cap);registerProcessor("om-play",Play);`;

function wsSend(b) { if (ws && ws.readyState === 1 && ready) { const o = new Uint8Array(1 + b.length); o[0] = 1; o.set(b, 1); ws.send(o); } }

async function startAudio() {
  if (ctx) return;
  if (!window.isSecureContext) throw new Error("microphone needs HTTPS (or localhost)");
  if (!("AudioEncoder" in window && "AudioDecoder" in window)) throw new Error("this browser has no WebCodecs Opus; use current Chrome/Edge on Android");
  const mic = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
  ctx = new AudioContext({ latencyHint: "interactive" });
  await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([WORKLET], { type: "application/javascript" })));
  cap = new AudioWorkletNode(ctx, "om-cap");
  player = new AudioWorkletNode(ctx, "om-play", { outputChannelCount: [1] });
  ctx.createMediaStreamSource(mic).connect(cap);
  player.connect(ctx.destination);
  enc = new AudioEncoder({ output: (chunk) => { const b = new Uint8Array(chunk.byteLength); chunk.copyTo(b); wsSend(ogg.packet(b, Math.round((chunk.duration || 20000) * 0.048))); }, error: (e) => log("encoder: " + e.message) });
  enc.configure({ codec: "opus", sampleRate: RATE, numberOfChannels: 1, bitrate: 32000, opus: { frameDuration: 20000 } });
  dec = new AudioDecoder({ output: (d) => { const f = new Float32Array(d.numberOfFrames); d.copyTo(f, { planeIndex: 0, format: "f32-planar" }); player.port.postMessage({ pcm: f.buffer, rate: d.sampleRate }, [f.buffer]); d.close(); }, error: (e) => log("decoder: " + e.message) });
  dec.configure({ codec: "opus", sampleRate: 48000, numberOfChannels: 1 });
  const bar = $("meter").firstChild;
  cap.port.onmessage = (e) => {
    let x = new Float32Array(e.data), pk = 0;
    for (let i = 0; i < x.length; i += 16) pk = Math.max(pk, Math.abs(x[i]));
    bar.style.width = Math.min(100, pk * 140) + "%";
    if (!ready) return;
    if ($("mode").value === "ptt" ? !pttDown : pttDown) x = new Float32Array(FRAME); // gate to silence (duplex: pttDown = muted)
    enc.encode(new AudioData({ format: "f32", sampleRate: RATE, numberOfFrames: FRAME, numberOfChannels: 1, timestamp: ts, data: x }));
    ts += 80000;
  };
  pill("st-mic", "mic: on", "ok");
}

const DECISION = ["verified", "uncertain", "mismatch", "listening"], DCLS = ["ok", "warn", "bad", ""];
const FALLBACK = ["GPU disabled", "GPU unreachable", "GPU busy", "upstream error"];

function onBinary(b) {
  const k = b[0], p = b.subarray(1);
  if (k === 0) {
    ready = true; pill("st-back", "backend: gpu", "ok"); log("relay ready (handshake)");
    ogg = oggWriter(); ts = 0; ogg.headers().forEach((h) => { const o = new Uint8Array(1 + h.length); o[0] = 1; o.set(h, 1); ws.send(o); });
    demux = oggReader((pkt) => { if (!isHeader(pkt) && dec && dec.state === "configured") dec.decode(new EncodedAudioChunk({ type: "key", timestamp: 0, data: pkt })); });
  } else if (k === 1) { if (demux) demux(p); }
  else if (k === 2 || k === 7) log("< " + new TextDecoder().decode(k === 7 ? p.subarray(1) : p));
  else if (k === 5) log("relay error: " + new TextDecoder().decode(p));
  else if (k === 0x40 && p.length >= 4) {
    const sc = p[0] === 1 ? ((p[1] << 8) | p[2]) << 16 >> 16 : null, d = p[3];
    pill("st-spk", "speaker: " + (DECISION[d] || "?") + (sc !== null ? " " + (sc / 1000).toFixed(2) : ""), DCLS[d]);
  } else if (k === 0x41) { ready = false; pill("st-back", "backend: text fallback (" + (FALLBACK[p[0]] || "?") + ")", "warn"); log("voice unavailable; use the text box (Galion One)"); }
  else if (k === 0x42) log("relay closed session: " + new TextDecoder().decode(p));
}

function connect() {
  if (!/^wss:\/\//.test(cfg.relay_url) && !/^ws:\/\/(localhost|127\.0\.0\.1)[:/]/.test(cfg.relay_url)) {
    log("set a wss:// relay URL in Settings first"); pill("st-conn", "no relay URL", "warn"); return;
  }
  pill("st-conn", "connecting", "warn"); ready = false;
  // Browsers cannot set an Authorization header on WebSockets; the token (if any)
  // rides as a second subprotocol. Draft: the relay must accept and not echo it.
  const protos = ["omvoice.v1"]; if (cfg.token) protos.push("bearer." + cfg.token);
  ws = new WebSocket(cfg.relay_url, protos);
  ws.binaryType = "arraybuffer";
  ws.onopen = () => { retry = 0; pill("st-conn", "connected", "ok"); log("connected; waiting for handshake"); $("talk").disabled = false; updateTalk(); };
  ws.onmessage = (e) => { if (typeof e.data !== "string") onBinary(new Uint8Array(e.data)); };
  ws.onclose = () => {
    ready = false; pill("st-conn", "offline", "bad"); $("talk").disabled = true; pill("st-back", "backend: ?", "");
    if (retry < 5 && $("connect").dataset.want === "1") { const d = 1000 * 2 ** retry++; log("reconnect in " + d / 1000 + "s"); setTimeout(connect, d); }
  };
  ws.onerror = () => log("websocket error");
}

$("connect").onclick = async () => {
  if (ws && ws.readyState <= 1) { $("connect").dataset.want = "0"; ws.close(); $("connect").textContent = "Connect"; return; }
  $("connect").dataset.want = "1"; $("connect").textContent = "Disconnect";
  try { await startAudio(); } catch (err) { log("audio: " + err.message); pill("st-mic", "mic: unavailable", "bad"); }
  connect();
};

function updateTalk() {
  const t = $("talk"), duplex = $("mode").value === "duplex";
  t.textContent = duplex ? (pttDown ? "Muted (tap)" : "Live (tap to mute)") : pttDown ? "Talking..." : "Hold to talk";
  t.className = duplex ? (pttDown ? "" : "live") : pttDown ? "on" : "";
}
$("mode").onchange = () => { pttDown = false; updateTalk(); };
const talk = $("talk");
talk.onpointerdown = (e) => {
  e.preventDefault(); if (ctx && ctx.state === "suspended") ctx.resume();
  if ($("mode").value === "duplex") { pttDown = !pttDown; updateTalk(); return; }
  pttDown = true; talk.setPointerCapture(e.pointerId); if (player) player.port.postMessage("flush"); updateTalk();
};
talk.onpointerup = talk.onpointercancel = () => { if ($("mode").value !== "ptt" || !pttDown) return; pttDown = false; updateTalk(); };

// Text path = Galion One (always; PersonaPlex takes no text input).
$("textform").onsubmit = async (e) => {
  e.preventDefault(); const text = $("text").value.trim(); if (!text) return; $("text").value = ""; log("> " + text);
  if (!cfg.text_url) return log("no text fallback URL set");
  try {
    const r = await fetch(cfg.text_url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ text, device_id: cfg.device_id || null }) });
    log("< " + (await r.text()).slice(0, 2000));
  } catch (err) { log("text fallback failed: " + err.message); }
};

if ("serviceWorker" in navigator && window.isSecureContext) navigator.serviceWorker.register("sw.js").catch(() => {});
if (!window.isSecureContext) log("not a secure context: the browser will refuse the microphone. Serve this page over HTTPS.");
loadConfig();
