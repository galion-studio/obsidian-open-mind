// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Small pure-Rust SQLite file-format integrity checker (no C, no deps).
//!
//! It follows the published file format (https://www.sqlite.org/fileformat.html)
//! and checks what `PRAGMA integrity_check` checks structurally: the header,
//! page count against the file size, every b-tree reachable from
//! `sqlite_schema` (page types, cell pointers inside the page, key order,
//! equal leaf depth, overflow chains), the freelist, ptrmap pages, and that
//! every page is used exactly once. A WAL file, when given, is validated
//! frame by frame (salts and cumulative checksums) and the committed frames
//! are applied on top of the main file, so the check sees what a reader of
//! the live database would see. Only counts and schema names are reported,
//! never row contents.
use std::collections::HashMap;

pub struct WalInfo {
    pub frames_total: u64,
    pub frames_valid: u64,
    pub commits: u64,
    pub db_pages_after_commit: u32,
    pub distinct_pages: usize,
    pub error: Option<String>,
}

pub struct Report {
    pub page_size: usize,
    pub header_pages: u32,
    pub file_pages: u64,
    pub pages: u32,
    pub freelist_header: u32,
    pub freelist_found: u32,
    pub btree_pages: u32,
    pub overflow_pages: u32,
    pub ptrmap_pages: u32,
    pub orphans: u32,
    /// (type, name, rootpage, entries)
    pub objects: Vec<(String, String, u32, u64)>,
    pub errors: Vec<String>,
    pub wal: Option<WalInfo>,
}

struct Db<'a> {
    main: &'a [u8],
    ps: usize,
    usable: usize,
    npages: u32,
    walpages: HashMap<u32, &'a [u8]>,
    used: Vec<u8>,
    errors: Vec<String>,
    btree_pages: u32,
    overflow_pages: u32,
}

fn be16(b: &[u8], o: usize) -> usize {
    ((b[o] as usize) << 8) | b[o + 1] as usize
}
fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// SQLite varint: up to 9 bytes, big-endian 7-bit groups, 9th byte full.
fn varint(b: &[u8], o: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let c = *b.get(o + i)?;
        if i == 8 {
            return Some(((v << 8) | c as u64, 9));
        }
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

const EMPTY: &[u8] = &[];

impl<'a> Db<'a> {
    fn page(&self, n: u32) -> &'a [u8] {
        if let Some(p) = self.walpages.get(&n) {
            return p;
        }
        let s = (n as usize - 1) * self.ps;
        self.main.get(s..s + self.ps).unwrap_or(EMPTY)
    }
    fn err(&mut self, e: String) {
        if self.errors.len() < 200 {
            self.errors.push(e);
        }
    }
    fn mark(&mut self, n: u32, what: &str) -> bool {
        if n == 0 || n > self.npages {
            self.err(format!("{what}: page {n} out of range 1..={}", self.npages));
            return false;
        }
        let u = &mut self.used[n as usize];
        if *u != 0 {
            self.err(format!("{what}: page {n} referenced twice"));
            return false;
        }
        *u = 1;
        true
    }

    /// Local payload size for a cell (file format 1.6 overflow rule).
    fn local(&self, p: u64, table_leaf: bool) -> usize {
        let u = self.usable as u64;
        let x = if table_leaf {
            u - 35
        } else {
            ((u - 12) * 64 / 255) - 23
        };
        if p <= x {
            return p as usize;
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((p - m) % (u - 4));
        if k <= x {
            k as usize
        } else {
            m as usize
        }
    }

    /// Payload bytes of one cell; follows and marks the overflow chain.
    fn payload(
        &mut self,
        pg: &'a [u8],
        off: usize,
        p: u64,
        table_leaf: bool,
        want: bool,
    ) -> Option<Vec<u8>> {
        let loc = self.local(p, table_leaf);
        if off + loc > self.usable {
            self.err(format!(
                "cell payload runs past the page end (off {off}, local {loc})"
            ));
            return None;
        }
        let mut out = if want {
            pg[off..off + loc].to_vec()
        } else {
            Vec::new()
        };
        if (loc as u64) < p {
            if off + loc + 4 > self.usable {
                self.err("overflow pointer past page end".into());
                return None;
            }
            let mut next = be32(pg, off + loc);
            let mut left = p - loc as u64;
            let mut guard = 0u32;
            while left > 0 {
                guard += 1;
                if guard > self.npages || !self.mark(next, "overflow") {
                    return None;
                }
                self.overflow_pages += 1;
                let op = self.page(next);
                if op.len() < self.ps {
                    self.err(format!("overflow page {next} unreadable"));
                    return None;
                }
                let take = left.min((self.usable - 4) as u64) as usize;
                if want {
                    out.extend_from_slice(&op[4..4 + take]);
                }
                left -= take as u64;
                next = be32(op, 0);
            }
            if next != 0 {
                self.err("overflow chain longer than the payload".into());
            }
        }
        Some(out)
    }

    /// Walk one b-tree. Returns entries; checks order and leaf depth.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        n: u32,
        table: bool,
        depth: u32,
        leaf_depth: &mut Option<u32>,
        last_rowid: &mut Option<i64>,
        records: &mut Option<Vec<Vec<u8>>>,
    ) -> u64 {
        if depth > 40 {
            self.err(format!("b-tree deeper than 40 at page {n}"));
            return 0;
        }
        if !self.mark(n, "btree") {
            return 0;
        }
        self.btree_pages += 1;
        let pg = self.page(n);
        if pg.len() < self.ps {
            self.err(format!("page {n} unreadable"));
            return 0;
        }
        let h = if n == 1 { 100 } else { 0 };
        let t = pg[h];
        let (leaf, ok) = match (t, table) {
            (13, true) | (10, false) => (true, true),
            (5, true) | (2, false) => (false, true),
            _ => (false, false),
        };
        if !ok {
            self.err(format!(
                "page {n}: bad b-tree page type {t} (expected {} b-tree)",
                if table { "table" } else { "index" }
            ));
            return 0;
        }
        let ncells = be16(pg, h + 3);
        let mut cstart = be16(pg, h + 5);
        if cstart == 0 {
            cstart = 65536;
        }
        let hl = if leaf { 8 } else { 12 };
        let ptr_end = h + hl + 2 * ncells;
        if ptr_end > cstart || cstart > self.usable {
            self.err(format!("page {n}: cell pointer array ({ncells} cells, ends {ptr_end}) overlaps content start {cstart}"));
            return 0;
        }
        // freeblock chain: ascending, inside the content area
        let mut fb = be16(pg, h + 1);
        let mut prev = 0usize;
        let mut g = 0;
        while fb != 0 {
            g += 1;
            if fb < cstart || fb + 4 > self.usable || fb <= prev || g > 4096 {
                self.err(format!("page {n}: bad freeblock at {fb}"));
                break;
            }
            prev = fb;
            fb = be16(pg, fb);
        }
        if leaf {
            match leaf_depth {
                None => *leaf_depth = Some(depth),
                Some(d) if *d != depth => {
                    let d = *d;
                    self.err(format!("page {n}: leaf at depth {depth}, others at {d}"))
                }
                _ => {}
            }
        }
        let mut entries = 0u64;
        for i in 0..ncells {
            let off = be16(pg, h + hl + 2 * i);
            if off < ptr_end || off + 4 > self.usable {
                self.err(format!(
                    "page {n}: cell {i} offset {off} outside content area"
                ));
                continue;
            }
            match (table, leaf) {
                (true, true) => {
                    let Some((p, a)) = varint(pg, off) else {
                        continue;
                    };
                    let Some((rid, b)) = varint(pg, off + a) else {
                        continue;
                    };
                    let rid = rid as i64;
                    if let Some(l) = *last_rowid {
                        if rid <= l {
                            self.err(format!("page {n}: rowid {rid} not after {l}"));
                        }
                    }
                    *last_rowid = Some(rid);
                    let want = records.is_some();
                    if let Some(pl) = self.payload(pg, off + a + b, p, true, want) {
                        if let Some(r) = records.as_mut() {
                            r.push(pl);
                        }
                    }
                    entries += 1;
                }
                (true, false) => {
                    let child = be32(pg, off);
                    entries += self.walk(child, true, depth + 1, leaf_depth, last_rowid, records);
                    if let Some((k, _)) = varint(pg, off + 4) {
                        if let Some(l) = *last_rowid {
                            if (k as i64) < l {
                                self.err(format!(
                                    "page {n}: interior key {k} below left subtree max {l}"
                                ));
                            }
                        }
                    }
                }
                (false, true) => {
                    let Some((p, a)) = varint(pg, off) else {
                        continue;
                    };
                    self.payload(pg, off + a, p, false, false);
                    entries += 1;
                }
                (false, false) => {
                    let child = be32(pg, off);
                    entries += self.walk(child, false, depth + 1, leaf_depth, last_rowid, records);
                    let Some((p, a)) = varint(pg, off + 4) else {
                        continue;
                    };
                    self.payload(pg, off + 4 + a, p, false, false);
                    entries += 1;
                }
            }
        }
        if !leaf {
            let right = be32(pg, h + 8);
            entries += self.walk(right, table, depth + 1, leaf_depth, last_rowid, records);
        }
        entries
    }
}

/// Decode the first five columns of a sqlite_schema record.
fn schema_row(r: &[u8]) -> Option<(String, String, u32, String)> {
    let (hlen, a) = varint(r, 0)?;
    let mut ho = a;
    let mut types = vec![];
    while ho < hlen as usize {
        let (t, b) = varint(r, ho)?;
        types.push(t);
        ho += b;
    }
    let mut body = hlen as usize;
    let mut cols: Vec<(u64, &[u8])> = vec![];
    for t in types {
        let len = match t {
            0 | 8 | 9 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 6,
            6 | 7 => 8,
            t if t >= 12 => ((t - 12) / 2) as usize,
            _ => return None,
        };
        cols.push((t, r.get(body..body + len)?));
        body += len;
    }
    let text = |i: usize| {
        cols.get(i)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    };
    let int = |i: usize| -> u32 {
        match cols.get(i) {
            Some((8, _)) => 0,
            Some((9, _)) => 1,
            Some((t, b)) if (1..=6).contains(t) => {
                let mut v: i64 = if b[0] & 0x80 != 0 { -1 } else { 0 };
                for x in b.iter() {
                    v = (v << 8) | *x as i64;
                }
                v as u32
            }
            _ => 0,
        }
    };
    Some((text(0), text(1), int(3), text(4)))
}

fn wal_sum(d: &[u8], big: bool, s: (u32, u32)) -> (u32, u32) {
    let (mut s0, mut s1) = s;
    for c in d.as_chunks::<8>().0 {
        let rd = |o: usize| {
            let a = [c[o], c[o + 1], c[o + 2], c[o + 3]];
            if big {
                u32::from_be_bytes(a)
            } else {
                u32::from_le_bytes(a)
            }
        };
        s0 = s0.wrapping_add(rd(0)).wrapping_add(s1);
        s1 = s1.wrapping_add(rd(4)).wrapping_add(s0);
    }
    (s0, s1)
}

/// Parse a WAL; returns info and the committed page images.
fn parse_wal(w: &[u8], ps: usize) -> (WalInfo, HashMap<u32, &[u8]>) {
    let mut info = WalInfo {
        frames_total: 0,
        frames_valid: 0,
        commits: 0,
        db_pages_after_commit: 0,
        distinct_pages: 0,
        error: None,
    };
    let mut map = HashMap::new();
    if w.is_empty() {
        return (info, map);
    }
    if w.len() < 32 {
        info.error = Some("wal shorter than its header".into());
        return (info, map);
    }
    let magic = be32(w, 0);
    if magic & !1 != 0x377f0682 {
        info.error = Some(format!("bad wal magic {magic:#x}"));
        return (info, map);
    }
    let big = magic & 1 == 1;
    let wps = be32(w, 8) as usize;
    if wps != ps {
        info.error = Some(format!("wal page size {wps} != db page size {ps}"));
        return (info, map);
    }
    let (salt1, salt2) = (be32(w, 16), be32(w, 20));
    let mut s = wal_sum(&w[..24], big, (0, 0));
    if s != (be32(w, 24), be32(w, 28)) {
        info.error = Some("wal header checksum mismatch".into());
        return (info, map);
    }
    let fsz = 24 + ps;
    info.frames_total = ((w.len() - 32) / fsz) as u64;
    let mut pending: Vec<(u32, &[u8])> = vec![];
    for i in 0..info.frames_total as usize {
        let f = &w[32 + i * fsz..32 + (i + 1) * fsz];
        if be32(f, 8) != salt1 || be32(f, 12) != salt2 {
            break;
        }
        let s2 = wal_sum(&f[24..], big, wal_sum(&f[..8], big, s));
        if s2 != (be32(f, 16), be32(f, 20)) {
            break;
        }
        s = s2;
        info.frames_valid += 1;
        pending.push((be32(f, 0), &f[24..]));
        let commit = be32(f, 4);
        if commit != 0 {
            info.commits += 1;
            info.db_pages_after_commit = commit;
            for (p, d) in pending.drain(..) {
                map.insert(p, d);
            }
        }
    }
    info.distinct_pages = map.len();
    (info, map)
}

pub fn check(main: &[u8], wal: Option<&[u8]>) -> Result<Report, String> {
    if main.len() < 100 || &main[..16] != b"SQLite format 3\0" {
        return Err("not an SQLite 3 database (bad magic)".into());
    }
    let ps = match be16(main, 16) {
        1 => 65536,
        p if p >= 512 && p.is_power_of_two() => p,
        p => return Err(format!("bad page size {p}")),
    };
    let reserved = main[20] as usize;
    let usable = ps - reserved;
    let header_pages = if be32(main, 24) == be32(main, 92) {
        be32(main, 28)
    } else {
        0
    };
    let file_pages = (main.len() / ps) as u64;
    let mut errors = vec![];
    if !main.len().is_multiple_of(ps) {
        errors.push(format!(
            "file size {} is not a multiple of the page size {ps}",
            main.len()
        ));
    }
    if header_pages != 0 && header_pages as u64 != file_pages {
        errors.push(format!(
            "header says {header_pages} pages, file has {file_pages}"
        ));
    }
    let (wal_info, walpages) = match wal {
        Some(w) => {
            let (i, m) = parse_wal(w, ps);
            (Some(i), m)
        }
        None => (None, HashMap::new()),
    };
    let mut npages = if header_pages != 0 {
        header_pages
    } else {
        file_pages as u32
    };
    if let Some(w) = &wal_info {
        if w.commits > 0 {
            npages = w.db_pages_after_commit;
        }
    }
    let mut db = Db {
        main,
        ps,
        usable,
        npages,
        walpages,
        used: vec![0; npages as usize + 1],
        errors,
        btree_pages: 0,
        overflow_pages: 0,
    };
    // page 1 header (possibly newer in the WAL)
    let p1 = db.page(1);
    let (fl_trunk, fl_count, autovac) = (be32(p1, 32), be32(p1, 36), be32(p1, 52));
    // schema
    let mut recs = Some(vec![]);
    let mut ld = None;
    let mut lr = None;
    let n_schema = db.walk(1, true, 0, &mut ld, &mut lr, &mut recs);
    let mut objects = vec![(
        "table".to_string(),
        "sqlite_schema".to_string(),
        1u32,
        n_schema,
    )];
    for r in recs.unwrap_or_default() {
        let Some((typ, name, root, sql)) = schema_row(&r) else {
            db.err("undecodable sqlite_schema record".into());
            continue;
        };
        if root == 0 {
            objects.push((typ, name, 0, 0));
            continue;
        }
        let table = typ == "table" && !sql.to_ascii_uppercase().contains("WITHOUT ROWID");
        let mut ld = None;
        let mut lr = None;
        let e = db.walk(root, table, 0, &mut ld, &mut lr, &mut None);
        objects.push((typ, name, root, e));
    }
    // freelist
    let mut found = 0u32;
    let mut trunk = fl_trunk;
    let mut g = 0;
    while trunk != 0 && g <= npages {
        g += 1;
        if !db.mark(trunk, "freelist trunk") {
            break;
        }
        found += 1;
        let tp = db.page(trunk);
        let nleaf = be32(tp, 4) as usize;
        if 8 + 4 * nleaf > usable {
            db.err(format!("freelist trunk {trunk}: {nleaf} leaves do not fit"));
            break;
        }
        for i in 0..nleaf {
            let l = be32(tp, 8 + 4 * i);
            if db.mark(l, "freelist leaf") {
                found += 1;
            }
        }
        trunk = be32(tp, 0);
    }
    if found != fl_count {
        db.err(format!("freelist: header count {fl_count}, found {found}"));
    }
    // ptrmap pages (auto-vacuum) and the lock-byte page
    let mut ptrmap = 0u32;
    if autovac != 0 {
        let step = (usable / 5) as u32 + 1;
        let mut p = 2u32;
        while p <= npages {
            if db.mark(p, "ptrmap") {
                ptrmap += 1;
            }
            p += step;
        }
    }
    let lock = (0x4000_0000 / ps) as u32 + 1;
    if lock <= npages {
        db.used[lock as usize] = 1;
    }
    let orphans = (1..=npages).filter(|&p| db.used[p as usize] == 0).count() as u32;
    if orphans > 0 {
        db.err(format!("{orphans} page(s) never used"));
    }
    Ok(Report {
        page_size: ps,
        header_pages,
        file_pages,
        pages: npages,
        freelist_header: fl_count,
        freelist_found: found,
        btree_pages: db.btree_pages,
        overflow_pages: db.overflow_pages,
        ptrmap_pages: ptrmap,
        orphans,
        objects,
        errors: db.errors,
        wal: wal_info,
    })
}
