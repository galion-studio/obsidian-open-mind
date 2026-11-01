// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Draws the Galion One home-screen icons (PNG) from the Galion mark
//! geometry (brand/galion/galion-mark.svg: an open orbit drawn as a G, its
//! bar pointing into a lit core). Original work, no third-party artwork.
//! Pure Rust: signed distances, 4x4 supersampling, PNG via miniz_oxide.
//!
//! Usage: cargo run -p galion-one --example pwa_icons -- crates/galion-one/pwa

use std::f64::consts::PI;

type Rgba = [f64; 4]; // straight colour 0..1, alpha 0..1

const BG: [u8; 3] = [0x05, 0x06, 0x0f]; // room sky top, also the manifest colours
const DISC: [u8; 3] = [0x0B, 0x0F, 0x14];
const RING: [u8; 3] = [0x24, 0x30, 0x3C];
const CORE: [u8; 3] = [0xB8, 0xE8, 0xFF];
const STOPS: [(f64, [u8; 3]); 3] = [
    (0.0, [0xB8, 0xE8, 0xFF]),
    (0.55, [0x58, 0xC4, 0xFF]),
    (1.0, [0x9D, 0x8C, 0xFF]),
];

fn rgb(c: [u8; 3], a: f64) -> Rgba {
    [
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
        a,
    ]
}

/// Centre and angles of the SVG arc `M47.6 18.4 A20 20 0 1 0 52 32`
/// (endpoint-to-centre conversion, SVG 1.1 F.6.5, with r = 20).
struct Arc {
    cx: f64,
    cy: f64,
    r: f64,
    a0: f64,
    sweep: f64, // signed, radians
}

fn arc() -> Arc {
    let (x1, y1, x2, y2, r) = (47.6f64, 18.4f64, 52.0f64, 32.0f64, 20.0f64);
    let (large, sweep_pos) = (true, false);
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let d2 = dx * dx + dy * dy;
    let mut k = ((r * r - d2) / d2).max(0.0).sqrt();
    if large == sweep_pos {
        k = -k;
    }
    let (cxp, cyp) = (k * dy, -k * dx);
    let (cx, cy) = (cxp + (x1 + x2) / 2.0, cyp + (y1 + y2) / 2.0);
    let a0 = (y1 - cy).atan2(x1 - cx);
    let a1 = (y2 - cy).atan2(x2 - cx);
    let mut sweep = a1 - a0;
    if !sweep_pos && sweep > 0.0 {
        sweep -= 2.0 * PI;
    }
    if sweep_pos && sweep < 0.0 {
        sweep += 2.0 * PI;
    }
    Arc {
        cx,
        cy,
        r,
        a0,
        sweep,
    }
}

fn seg_dist(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (vx, vy) = (bx - ax, by - ay);
    let t = (((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((px - ax - t * vx).powi(2) + (py - ay - t * vy).powi(2)).sqrt()
}

fn arc_dist(a: &Arc, px: f64, py: f64) -> f64 {
    let ang = (py - a.cy).atan2(px - a.cx);
    // Offset from a0 in the sweep direction, 0..2pi.
    let mut off = if a.sweep < 0.0 {
        a.a0 - ang
    } else {
        ang - a.a0
    };
    off = off.rem_euclid(2.0 * PI);
    if off <= a.sweep.abs() {
        return (((px - a.cx).powi(2) + (py - a.cy).powi(2)).sqrt() - a.r).abs();
    }
    let end = |t: f64| (a.cx + a.r * t.cos(), a.cy + a.r * t.sin());
    let (sx, sy) = end(a.a0);
    let (ex, ey) = end(a.a0 + a.sweep);
    ((px - sx).hypot(py - sy)).min((px - ex).hypot(py - ey))
}

fn gradient(x: f64, y: f64) -> [u8; 3] {
    // objectBoundingBox of the G path, diagonal x1=y1=0 -> x2=y2=1.
    let (bx0, by0, bx1, by1) = (12.0, 12.0, 52.0, 52.0);
    let u = ((x - bx0) / (bx1 - bx0)).clamp(0.0, 1.0);
    let v = ((y - by0) / (by1 - by0)).clamp(0.0, 1.0);
    let t = ((u + v) / 2.0).clamp(0.0, 1.0);
    let mut i = 0;
    while i + 2 < STOPS.len() && t > STOPS[i + 1].0 {
        i += 1;
    }
    let (t0, c0) = STOPS[i];
    let (t1, c1) = STOPS[i + 1];
    let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * f).round() as u8;
    [mix(c0[0], c1[0]), mix(c0[1], c1[1]), mix(c0[2], c1[2])]
}

fn over(dst: Rgba, src: Rgba) -> Rgba {
    let a = src[3] + dst[3] * (1.0 - src[3]);
    if a <= 0.0 {
        return [0.0; 4];
    }
    let ch = |i: usize| (src[i] * src[3] + dst[i] * dst[3] * (1.0 - src[3])) / a;
    [ch(0), ch(1), ch(2), a]
}

/// One sample in mark units (0..64).
fn sample(a: &Arc, x: f64, y: f64, bg: bool) -> Rgba {
    let mut c = if bg { rgb(BG, 1.0) } else { [0.0; 4] };
    let d = (x - 32.0).hypot(y - 32.0);
    if d <= 30.0 {
        c = over(c, rgb(DISC, 1.0));
    }
    if (d - 30.0).abs() <= 0.75 {
        c = over(c, rgb(RING, 1.0));
    }
    let g = arc_dist(a, x, y).min(seg_dist(x, y, 52.0, 32.0, 34.0, 32.0));
    if g <= 2.5 {
        c = over(c, rgb(gradient(x, y), 1.0));
    }
    if d <= 4.2 {
        c = over(c, rgb(CORE, 1.0));
    }
    c
}

/// `size` px square; the 64-unit mark is drawn `mark` px wide, centred.
fn render(size: u32, mark: f64, bg: bool) -> Vec<u8> {
    let a = arc();
    let n = 4u32;
    let scale = 64.0 / mark;
    let off = (f64::from(size) - mark) / 2.0;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f64; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let fx = f64::from(px) + (f64::from(sx) + 0.5) / f64::from(n);
                    let fy = f64::from(py) + (f64::from(sy) + 0.5) / f64::from(n);
                    let s = sample(&a, (fx - off) * scale, (fy - off) * scale, bg);
                    for i in 0..3 {
                        acc[i] += s[i] * s[3];
                    }
                    acc[3] += s[3];
                }
            }
            let k = f64::from(n * n);
            let alpha = acc[3] / k;
            for v in acc.iter().take(3) {
                let ch = if acc[3] > 0.0 { v / acc[3] } else { 0.0 };
                out.push((ch * 255.0).round().clamp(0.0, 255.0) as u8);
            }
            out.push((alpha * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    let row = (size * 4) as usize;
    let mut raw = Vec::with_capacity(rgba.len() + size as usize);
    for y in 0..size as usize {
        raw.push(1); // Sub filter
        let line = &rgba[y * row..(y + 1) * row];
        for x in 0..row {
            let left = if x >= 4 { line[x - 4] } else { 0 };
            raw.push(line[x].wrapping_sub(left));
        }
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 10),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/galion-one/pwa".to_string());
    // (file, size, mark width, opaque background)
    let set: [(&str, u32, f64, bool); 5] = [
        ("icon-192.png", 192, 192.0, false),
        ("icon-512.png", 512, 512.0, false),
        // Maskable: the whole mark inside the 80% safe circle.
        ("maskable-192.png", 192, 192.0 * 0.74, true),
        ("maskable-512.png", 512, 512.0 * 0.74, true),
        // iOS rounds the corners itself and turns transparency black.
        ("apple-touch-icon.png", 180, 180.0 * 0.8, true),
    ];
    for (name, size, mark, bg) in set {
        let bytes = png(size, &render(size, mark, bg));
        let path = format!("{}/{name}", dir.trim_end_matches('/'));
        std::fs::write(&path, &bytes).expect("write icon");
        println!("{path} {} bytes", bytes.len());
    }
}
