// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//
// Port of one/trace.py: look at a black mark, then write one filled SVG of its
// corners. Same steps as the Python: grey threshold < 140, Moore-neighbour
// contour walk from the first black pixel, subsample to <= 800 points,
// Ramer-Douglas-Peucker with growing epsilon until 5-8 corners, then a
// 100x160 viewBox polygon with the vision caption as an XML comment.
//
// Input: binary PGM (P5) or PPM (P6), decoded here with std only. The Python
// used Pillow for JPEG/PNG; to keep this dependency-free and portable,
// convert first (any tool, e.g. `convert mark.jpg mark.pgm`). Not shipped:
// nothing calls it (docs/legacy-python.md); it is an operator tool.

pub struct Grey {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// Decode binary PGM/PPM. PPM is turned to grey like Pillow's "L" mode.
pub fn decode_pnm(b: &[u8]) -> Result<Grey, String> {
    let mut i = 0usize;
    let mut fields = Vec::new();
    while fields.len() < 4 {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'#') {
            if b[i] == b'#' {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
        let s = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'#' {
            i += 1;
        }
        if s == i {
            return Err("short pnm header".into());
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    i += 1; // the single whitespace byte before the raster
    let num = |k: usize| fields[k].parse::<usize>().map_err(|_| "bad pnm number");
    let (w, h, max) = (num(1)?, num(2)?, num(3)?);
    if max == 0 || max > 255 {
        return Err("only 8-bit pnm is supported".into());
    }
    let chans = match fields[0].as_str() {
        "P5" => 1,
        "P6" => 3,
        _ => return Err("not a binary PGM/PPM (P5/P6)".into()),
    };
    let need = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(chans))
        .ok_or("too big")?;
    let raster = b.get(i..i + need).ok_or("pnm raster too short")?;
    let px = if chans == 1 {
        raster.to_vec()
    } else {
        raster
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| {
                ((c[0] as u32 * 19595 + c[1] as u32 * 38470 + c[2] as u32 * 7471 + 0x8000) >> 16)
                    as u8
            })
            .collect()
    };
    Ok(Grey { w, h, px })
}

type Pt = (i64, i64);

pub fn corners(g: &Grey) -> Result<Vec<(f64, f64)>, String> {
    let (w, h) = (g.w as i64, g.h as i64);
    let black = |x: i64, y: i64| g.px[(y * w + x) as usize] < 140;
    let start = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .find(|&(x, y)| black(x, y))
        .ok_or("no mark")?;
    const NEIGH: [Pt; 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    let (mut cx, mut cy) = start;
    let mut prev = (cx, cy - 1);
    let mut contour = vec![start];
    for _ in 0..(w * h) {
        let index = NEIGH
            .iter()
            .position(|&d| d == (prev.0 - cx, prev.1 - cy))
            .unwrap_or(0);
        let mut nxt = None;
        for turn in 1..9 {
            let (ox, oy) = NEIGH[(index + turn) % 8];
            let (nx, ny) = (cx + ox, cy + oy);
            if (0..w).contains(&nx) && (0..h).contains(&ny) && black(nx, ny) {
                nxt = Some((nx, ny));
                break;
            }
        }
        let Some(n) = nxt else { break };
        prev = (cx, cy);
        (cx, cy) = n;
        if (cx, cy) == start && contour.len() > 8 {
            break;
        }
        contour.push((cx, cy));
    }
    let step = (contour.len() / 800).max(1);
    let reduced: Vec<Pt> = contour.iter().step_by(step).copied().collect();
    let mut chosen = reduced.clone();
    for eps in [6.0, 10.0, 16.0, 24.0, 36.0, 52.0] {
        chosen = rdp(&reduced, eps);
        if (5..=8).contains(&chosen.len()) {
            break;
        }
    }
    if let (Some(f), Some(l)) = (chosen.first(), chosen.last()) {
        if (f.0 - l.0).abs() < 4 && (f.1 - l.1).abs() < 4 {
            chosen.pop();
        }
    }
    Ok(chosen.iter().map(|&(x, y)| (x as f64, y as f64)).collect())
}

pub fn rdp(points: &[Pt], eps: f64) -> Vec<Pt> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (s, e) = (points[0], points[points.len() - 1]);
    let (sx, sy, ex, ey) = (s.0 as f64, s.1 as f64, e.0 as f64, e.1 as f64);
    let mut span = ((ex - sx).powi(2) + (ey - sy).powi(2)).sqrt();
    if span == 0.0 {
        span = 1.0;
    }
    let (mut far, mut index) = (0.0f64, 0usize);
    for (i, &(x, y)) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let (x, y) = (x as f64, y as f64);
        let dist = ((ey - sy) * x - (ex - sx) * y + ex * sy - ey * sx).abs() / span;
        if dist > far {
            far = dist;
            index = i;
        }
    }
    if far > eps {
        let mut left = rdp(&points[..=index], eps);
        let right = rdp(&points[index..], eps);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![s, e]
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn svg(points: &[(f64, f64)], caption: &str) -> String {
    let fold = |f: fn(f64, f64) -> f64, init: f64, sel: fn(&(f64, f64)) -> f64| {
        points.iter().map(sel).fold(init, f)
    };
    let (minx, maxx) = (
        fold(f64::min, f64::MAX, |p| p.0),
        fold(f64::max, f64::MIN, |p| p.0),
    );
    let (miny, maxy) = (
        fold(f64::min, f64::MAX, |p| p.1),
        fold(f64::max, f64::MIN, |p| p.1),
    );
    let width = (maxx - minx).max(1.0);
    let height = (maxy - miny).max(1.0);
    let pad = width.max(height) * 0.04;
    let coords: Vec<String> = points
        .iter()
        .map(|&(x, y)| {
            format!(
                "{:?},{:?}",
                round2((x - minx + pad) / (width + pad * 2.0) * 100.0),
                round2((y - miny + pad) / (height + pad * 2.0) * 160.0)
            )
        })
        .collect();
    let note = caption.replace("--", " ").replace('<', "");
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 160\" fill=\"#111\">\n<!-- {note} -->\n<polygon points=\"{}\"/>\n</svg>\n",
        coords.join(" ")
    )
}

/// multipart body the Python sent to the eye's /see.
pub fn see_body(name: &str, image: &[u8]) -> (String, Vec<u8>) {
    let b = "markbound";
    let mut body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: image/jpeg\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(image);
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Grey {
        // 40x40 white, black filled square from (10,10) to (29,29)
        let (w, h) = (40, 40);
        let mut px = vec![255u8; w * h];
        for y in 10..30 {
            for x in 10..30 {
                px[y * w + x] = 0;
            }
        }
        Grey { w, h, px }
    }

    #[test]
    fn pnm_decode() {
        let mut pgm = b"P5\n# c\n2 1\n255\n".to_vec();
        pgm.extend_from_slice(&[0, 200]);
        let g = decode_pnm(&pgm).unwrap();
        assert_eq!((g.w, g.h, g.px.clone()), (2, 1, vec![0, 200]));
        let mut ppm = b"P6 1 1 255 ".to_vec();
        ppm.extend_from_slice(&[255, 255, 255]);
        assert_eq!(decode_pnm(&ppm).unwrap().px, vec![255]);
        assert!(decode_pnm(b"P2 1 1 255 0").is_err());
    }

    #[test]
    fn square_has_four_corners_and_svg() {
        let pts = corners(&square()).unwrap();
        assert_eq!(pts.len(), 4, "{pts:?}");
        for c in [(10.0, 10.0), (29.0, 10.0), (29.0, 29.0), (10.0, 29.0)] {
            assert!(pts.contains(&c), "{pts:?}");
        }
        let s = svg(&pts, "a <b> -- mark");
        assert!(s.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 160\" fill=\"#111\">\n<!-- a b>   mark -->\n<polygon points=\""));
        assert!(s.contains("3.7,5.93"), "{s}");
    }

    #[test]
    fn blank_is_no_mark() {
        let g = Grey {
            w: 3,
            h: 3,
            px: vec![255; 9],
        };
        assert!(corners(&g).is_err());
    }

    #[test]
    fn rdp_line_collapses() {
        let line: Vec<Pt> = (0..20).map(|i| (i, 0)).collect();
        assert_eq!(rdp(&line, 1.0), vec![(0, 0), (19, 0)]);
    }
}
