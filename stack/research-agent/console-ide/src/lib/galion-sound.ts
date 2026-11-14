// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

// Minimal Galion sound set for the TypeScript console.
// Pitches match crates/console-ui/src/sound.rs: 432 Hz times a just ratio.
// Admin cues are approval, task done and error. Bell strikes are only for
// a window opening, closing, minimising or settling.
// Resonance is on unless localStorage galion-resonance is "0".
// Silence is opt-in: localStorage galion-silence is "1".

export const TONIC_HZ = 432;

export const JUST = {
  unison: [1, 1],
  octaveBelow: [1, 2],
  minorSecond: [16, 15],
  majorThird: [5, 4],
  fourth: [4, 3],
  fifth: [3, 2],
  octave: [2, 1],
} as const;

// Just ratios of the bell's own fundamental (not extra cue notes).
const BELL_PARTIALS: [number, number][] = [
  [1, 1],
  [3, 2],
  [2, 1],
  [5, 2],
  [3, 1],
  [4, 1],
];

type CueName = "approval" | "done" | "error";
type BellName = "open" | "close" | "minimise" | "settle";

const CUE_NOTES: Record<CueName, (keyof typeof JUST)[]> = {
  approval: ["fourth", "fifth"],
  done: ["fifth", "octave"],
  error: ["minorSecond", "minorSecond"],
};

const BELL_FUNDAMENTAL: Record<BellName, keyof typeof JUST> = {
  open: "unison",
  close: "octaveBelow",
  minimise: "octave",
  settle: "majorThird",
};

const SILENCE_KEY = "galion-silence";
const RESONANCE_KEY = "galion-resonance";

export function hz(name: keyof typeof JUST): number {
  const [num, den] = JUST[name];
  return (TONIC_HZ * num) / den;
}

export function silenced(): boolean {
  try {
    return localStorage.getItem(SILENCE_KEY) === "1";
  } catch {
    return false;
  }
}

export function setSilence(on: boolean): void {
  try {
    localStorage.setItem(SILENCE_KEY, on ? "1" : "0");
  } catch {
    /* storage blocked; the next cue still checks silenced() */
  }
}

function resonanceOn(): boolean {
  try {
    return localStorage.getItem(RESONANCE_KEY) !== "0";
  } catch {
    return true;
  }
}

function ctx(): AudioContext | null {
  const Ctor = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  const g = window as unknown as { __galionEar?: AudioContext };
  if (!g.__galionEar) g.__galionEar = new Ctor();
  void g.__galionEar.resume();
  return g.__galionEar;
}

function tone(context: AudioContext, frequency: number, when: number, dur: number, gain: number, strike: boolean): void {
  const osc = context.createOscillator();
  const amp = context.createGain();
  osc.frequency.value = frequency;
  osc.type = "sine";
  const attack = strike ? 0.003 : 0.012;
  amp.gain.setValueAtTime(0.0001, when);
  amp.gain.exponentialRampToValueAtTime(Math.max(0.001, gain), when + attack);
  amp.gain.exponentialRampToValueAtTime(0.0001, when + dur);
  osc.connect(amp);
  amp.connect(context.destination);
  osc.start(when);
  osc.stop(when + dur + 0.02);
}

/** Admin cue. Never a bell strike. */
export function cue(name: CueName): void {
  if (silenced()) return;
  const context = ctx();
  if (!context) return;
  const now = context.currentTime;
  const notes = CUE_NOTES[name];
  notes.forEach((ratio, i) => {
    tone(context, hz(ratio), now + i * 0.22, 0.34, 0.12, false);
  });
  if (resonanceOn()) tone(context, hz("unison"), now, 0.9, 0.04, false);
}

/** Window bell. Only open, close, minimise and settle. */
export function bell(name: BellName): void {
  if (silenced()) return;
  const context = ctx();
  if (!context) return;
  const now = context.currentTime;
  const fund = hz(BELL_FUNDAMENTAL[name]);
  const gains = [0.16, 0.09, 0.055, 0.03, 0.02, 0.012];
  BELL_PARTIALS.forEach(([num, den], i) => {
    tone(context, fund * (num / den), now, name === "minimise" ? 0.28 : 0.7, gains[i] ?? 0.02, true);
  });
  if (resonanceOn()) tone(context, fund, now, 1.1, 0.04, false);
}
