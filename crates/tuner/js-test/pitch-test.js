// Offline test of www/pitch.js with synthetic sine and plucked tones.
const P = require('../www/pitch.js');
const SR = 48000, N = 2048;
function sine(f, phase = 0.3) {
  const b = new Float32Array(N);
  for (let i = 0; i < N; i++) b[i] = 0.5 * Math.sin(2 * Math.PI * f * i / SR + phase);
  return b;
}
let seed = 12345; const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 4294967296);
// Plucked-like: harmonics 1..8 with 1/h^1.2 amplitudes, slight inharmonicity, decay, noise.
// offset = samples after the pluck when the analysis window starts.
function pluck(f, offset = 2400, weakFund = false) {
  const b = new Float32Array(N), B = 0.00002; // nylon strings: very low inharmonicity
  for (let i = 0; i < N; i++) {
    const t = (i + offset) / SR; let s = 0;
    for (let h = 1; h <= 8; h++) {
      const fh = f * h * Math.sqrt((1 + B * h * h) / (1 + B));
      let a = 1 / Math.pow(h, 1.2); if (weakFund && h === 1) a = 0.35;
      s += a * Math.exp(-t * (2 + 1.5 * h)) * Math.sin(2 * Math.PI * fh * t + h);
    }
    b[i] = 0.3 * s + 0.01 * (rnd() - 0.5);
  }
  return b;
}
const targets = [['G3', 196.00], ['C4', 261.63], ['E4', 329.63], ['G4', 392.00], ['A4', 440.00]];
let pass = 0, fail = 0;
console.log('wave   expect detune  freqHz    string  cents   inTune  ok');
for (const [id, f0] of targets) for (const det of [-15, 0, 15]) for (const kind of ['sine', 'pluck', 'pluck-weakfund']) {
  const f = f0 * Math.pow(2, det / 1200);
  const buf = kind === 'sine' ? sine(f) : pluck(f, 2400, kind === 'pluck-weakfund');
  const r = P.detect(buf, SR);
  let ok = false, line;
  if (!r) line = 'no pitch';
  else {
    const id2 = P.identify(r.freq);
    // pluck uses slightly inharmonic partials, allow 3 cents; sine 1 cent
    const tol = kind === 'sine' ? 1 : 3;
    ok = id2.string.id === id && Math.abs(id2.cents - det) <= tol && id2.inTune === (det === 0);
    line = `${r.freq.toFixed(2).padStart(8)}  ${id2.string.id.padEnd(6)} ${id2.cents.toFixed(2).padStart(6)}  ${String(id2.inTune).padEnd(6)}`;
  }
  ok ? pass++ : fail++;
  console.log(`${kind.padEnd(14)} ${id} ${String(det).padStart(4)}c  ${line}  ${ok ? 'PASS' : 'FAIL'}`);
}
// Silence must give no reading.
const silent = P.detect(new Float32Array(N), SR); silent === null ? pass++ : fail++;
console.log('silence -> ' + (silent === null ? 'null PASS' : 'FAIL'));
// Timing: CPU cost per analysis frame.
const tb = pluck(196), T0 = process.hrtime.bigint(); for (let i = 0; i < 500; i++) P.detect(tb, SR);
console.log(`avg detect() time: ${(Number(process.hrtime.bigint() - T0) / 500 / 1e6).toFixed(3)} ms per 2048-sample frame`);
console.log(`RESULT: ${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
