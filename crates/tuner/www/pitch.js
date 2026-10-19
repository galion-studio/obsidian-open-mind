/* Ukulele pitch detection: McLeod Pitch Method (NSDF), plain JS, no dependencies.
   Works in the browser (window.Pitch) and in node (module.exports). */
(function (root) {
  'use strict';
  var A4 = 440;
  // Standard GCEA (re-entrant) plus Low-G. Listed by pitch, high to low.
  function hz(semisFromA4) { return A4 * Math.pow(2, semisFromA4 / 12); }
  var STRINGS = [
    { id: 'A4', name: 'A', note: 'A4', hz: hz(0),   pos: 4, label: 'A string (1st)' },
    { id: 'G4', name: 'G', note: 'G4', hz: hz(-2),  pos: 1, label: 'G string (4th, high/re-entrant G)' },
    { id: 'E4', name: 'E', note: 'E4', hz: hz(-5),  pos: 3, label: 'E string (2nd)' },
    { id: 'C4', name: 'C', note: 'C4', hz: hz(-9),  pos: 2, label: 'C string (3rd)' },
    { id: 'G3', name: 'G', note: 'G3', hz: hz(-14), pos: 1, label: 'G string (4th, Low-G)' }
  ];
  var NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

  // Reusable work buffer to stay allocation-free in the audio loop.
  var nsdfBuf = null;

  /* Returns {freq, clarity} or null. buf: Float32Array of samples. */
  function detect(buf, sampleRate, opts) {
    opts = opts || {};
    var minHz = opts.minHz || 150, maxHz = opts.maxHz || 700;
    var k = opts.k || 0.93, minClarity = opts.minClarity || 0.8, minRms = opts.minRms || 0.008;
    var n = buf.length, i, tau;
    var rms = 0;
    for (i = 0; i < n; i++) rms += buf[i] * buf[i];
    rms = Math.sqrt(rms / n);
    if (rms < minRms) return null;
    var maxTau = Math.min(Math.ceil(sampleRate / minHz) + 2, (n >> 1));
    var minTau = Math.floor(sampleRate / maxHz);
    if (!nsdfBuf || nsdfBuf.length < maxTau + 1) nsdfBuf = new Float64Array(maxTau + 1);
    var nsdf = nsdfBuf;
    // m'(tau) computed incrementally: m(tau) = sum_{i<n-tau} x[i]^2 + x[i+tau]^2
    var m = 2 * rms * rms * n;
    nsdf[0] = 1;
    for (tau = 1; tau <= maxTau; tau++) {
      m -= buf[n - tau] * buf[n - tau] + buf[tau - 1] * buf[tau - 1];
      var acf = 0, lim = n - tau;
      for (i = 0; i < lim; i++) acf += buf[i] * buf[i + tau];
      nsdf[tau] = m > 0 ? 2 * acf / m : 0;
    }
    // Peak picking: key maxima between positive zero crossings.
    var peaks = [], best = -1, t = 1;
    while (t < maxTau && nsdf[t] > 0) t++;           // skip the zero-lag lobe
    while (t < maxTau) {
      while (t < maxTau && nsdf[t] <= 0) t++;
      var pk = -1, pv = -Infinity;
      while (t < maxTau && nsdf[t] > 0) {
        if (nsdf[t] > pv) { pv = nsdf[t]; pk = t; }
        t++;
      }
      if (pk > 0 && pk < maxTau) { peaks.push(pk); if (pv > best) best = pv; }
    }
    if (!peaks.length || best < minClarity) return null;
    var thresh = k * best, chosen = -1;
    for (i = 0; i < peaks.length; i++) {
      if (peaks[i] >= minTau && nsdf[peaks[i]] >= thresh) { chosen = peaks[i]; break; }
    }
    if (chosen < 0) return null;
    // Parabolic interpolation around the chosen peak.
    var a = nsdf[chosen - 1], b = nsdf[chosen], c = nsdf[chosen + 1];
    var den = a - 2 * b + c, shift = den !== 0 ? 0.5 * (a - c) / den : 0;
    var period = chosen + shift;
    var freq = sampleRate / period;
    if (freq < minHz * 0.97 || freq > maxHz) return null;
    return { freq: freq, clarity: b - 0.25 * (a - c) * shift, rms: rms };
  }

  function cents(f, ref) { return 1200 * Math.log(f / ref) / Math.LN2; }

  /* Fully automatic string identification: nearest standard string in cents. */
  function identify(freq) {
    var bestS = null, bestC = Infinity;
    for (var i = 0; i < STRINGS.length; i++) {
      var c = cents(freq, STRINGS[i].hz);
      if (Math.abs(c) < Math.abs(bestC)) { bestC = c; bestS = STRINGS[i]; }
    }
    var midi = 69 + 12 * Math.log(freq / A4) / Math.LN2, r = Math.round(midi);
    return {
      string: bestS, cents: bestC, inTune: Math.abs(bestC) <= 5,
      // farther than ~2 semitones from any ukulele string: probably not tuning a string
      offString: Math.abs(bestC) > 250,
      nearestNote: NOTE_NAMES[((r % 12) + 12) % 12] + (Math.floor(r / 12) - 1),
      noteCents: (midi - r) * 100
    };
  }

  var api = { detect: detect, identify: identify, cents: cents, STRINGS: STRINGS, A4: A4 };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Pitch = api;
})(this);
