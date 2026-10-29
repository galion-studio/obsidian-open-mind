// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// /hermes dictation AudioWorklet (owner order t362u): mono input at the
// context rate -> 16 kHz PCM16 little-endian, posted as 50 ms frames
// (800 samples = 1600 bytes) to the page, which sends them to galion-stt
// over /api/stt/stream. Linear resampling after a light low-pass.
"use strict";
var OUT_RATE = 16000;
var FRAME = 800;

class GalionPcm16 extends AudioWorkletProcessor {
  constructor() {
    super();
    this.step = sampleRate / OUT_RATE; // input samples per output sample
    // One-pole low-pass near the new Nyquist to limit aliasing.
    var fc = Math.min(7200, sampleRate / 2 - 1);
    this.alpha = 1 - Math.exp((-2 * Math.PI * fc) / sampleRate);
    this.lp = 0;
    this.prev = 0; // last filtered sample of the previous block (index -1)
    this.t = 0; // next output position, in input samples, block-relative
    this.frame = new Int16Array(FRAME);
    this.n = 0;
    this.on = true;
    var self = this;
    this.port.onmessage = function (e) { if (e.data === "stop") self.on = false; };
  }

  push(v) {
    if (v > 1) v = 1; else if (v < -1) v = -1;
    this.frame[this.n++] = v < 0 ? v * 0x8000 : v * 0x7fff;
    if (this.n === FRAME) {
      var out = this.frame.buffer; // Int16Array is little-endian on every browser platform
      this.port.postMessage(out, [out]);
      this.frame = new Int16Array(FRAME);
      this.n = 0;
    }
  }

  process(inputs) {
    if (!this.on) return false;
    var ch = inputs[0] && inputs[0][0];
    if (!ch || !ch.length) return true;
    var len = ch.length;
    var x = new Float32Array(len);
    var lp = this.lp;
    for (var i = 0; i < len; i++) { lp += this.alpha * (ch[i] - lp); x[i] = lp; }
    this.lp = lp;
    if (this.step === 1) {
      for (var k = 0; k < len; k++) this.push(x[k]);
      return true;
    }
    // Position -1 is this.prev; interpolate between floor(t) and floor(t)+1.
    while (this.t < len - 1) {
      var j = Math.floor(this.t);
      var f = this.t - j;
      var a = j < 0 ? this.prev : x[j];
      var b = x[j + 1];
      this.push(a + (b - a) * f);
      this.t += this.step;
    }
    this.t -= len;
    this.prev = x[len - 1];
    return true;
  }
}

registerProcessor("galion-pcm16", GalionPcm16);
