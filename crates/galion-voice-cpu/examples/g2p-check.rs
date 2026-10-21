// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! G2P check: CMUdict load time / RSS, sample phonemisation, and how well the
//! letter-to-sound rules do on dictionary words (phone error rate, stress
//! ignored), as a proxy for out-of-vocabulary quality.
//!
//! usage: g2p-check <cmudict.dict> [text]

use std::time::Instant;

use galion_voice_cpu::g2p::{
    kitten_ids, kitten_symbol_table, letter_to_sound, Lexicon, Phonemizer,
};

fn vm_hwm_kb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM"))
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        })
        .unwrap_or_else(|| "?".into())
}

fn edit_distance(a: &[&str], b: &[&str]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(x != y))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn main() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let path = a.next().ok_or("usage: g2p-check <cmudict.dict> [text]")?;
    let t = Instant::now();
    let lex = Lexicon::load(std::path::Path::new(&path))?;
    println!(
        "lexicon: {} words, load {} ms, VmHWM {} kB",
        lex.len(),
        t.elapsed().as_millis(),
        vm_hwm_kb()
    );

    // Letter-to-sound vs dictionary on every 50th plain alphabetic word.
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let (mut words, mut exact, mut errs, mut total) = (0usize, 0usize, 0usize, 0usize);
    for line in text.lines().step_by(50) {
        let mut it = line.split_whitespace();
        let Some(w) = it.next() else { continue };
        if !w.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        let truth: Vec<&str> = it
            .take_while(|t| !t.starts_with('#'))
            .map(|t| t.trim_end_matches(['0', '1', '2']))
            .collect();
        let guess_ph = letter_to_sound(w);
        let guess: Vec<&str> = guess_ph.iter().map(|p| p.code()).collect();
        let d = edit_distance(&guess, &truth);
        words += 1;
        exact += usize::from(d == 0);
        errs += d;
        total += truth.len();
    }
    println!(
        "letter-to-sound on {words} dictionary words: exact {:.1} %, phone error rate {:.1} %",
        100.0 * exact as f64 / words.max(1) as f64,
        100.0 * errs as f64 / total.max(1) as f64
    );

    let mut g = Phonemizer::new(lex);
    g.add_word("galion", "G AE1 L IY0 AH0 N");
    let table = kitten_symbol_table();
    let samples: Vec<String> = match a.next() {
        Some(t) => vec![t],
        None => vec![
            "Hello, world.".into(),
            "This high quality TTS model works without a GPU.".into(),
            "Obsidian Open Mind, made by Galion, is listening.".into(),
            "And so my fellow Americans, ask not what your country can do for you.".into(),
            "It costs 115 dollars in 2026; that's 12% more!".into(),
        ],
    };
    for s in &samples {
        let t = Instant::now();
        let ipa = g.phonemize(s);
        let us = t.elapsed().as_micros();
        let ids = kitten_ids(&ipa, &table);
        println!(
            "{s:?}\n  -> {ipa}\n  {} ids, {us} us, {:?}",
            ids.len(),
            g.stats
        );
    }
    println!("VmHWM {} kB", vm_hwm_kb());
    Ok(())
}
