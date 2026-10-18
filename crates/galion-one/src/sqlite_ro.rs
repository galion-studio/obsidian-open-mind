// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//
// A tiny read-only SQLite file reader, pure Rust, no C library. It replaces
// the hand-written libsqlite3 FFI that galion-one used for guardian.db: that
// FFI linked the system libsqlite3.so.0, which does not exist for the musl,
// aarch64, armv7 and Android targets (UNIVERSAL-CPU.md). It also opened the
// file read-write (sqlite3_open), which could create an empty guardian.db;
// this reader only ever opens the file with File::open.
//
// Scope, on purpose small: rowid tables only (no WITHOUT ROWID), UTF-8 text
// encoding, full table scans, overflow pages. Indexes are never consulted.
// WAL-mode databases are read through their committed -wal frames (salts and
// checksums checked; the -shm index is not needed). The reader takes no
// lock, so a checkpoint racing a read can make one read fail; callers treat
// any error as "no data this tick".

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    /// SQLite `CAST(x AS INT)`: real truncates toward zero, text takes its
    /// leading integer, NULL stays NULL (None).
    pub fn cast_int(&self) -> Option<i64> {
        match self {
            Value::Null => None,
            Value::Int(i) => Some(*i),
            Value::Real(r) => Some(if r.is_nan() { 0 } else { *r as i64 }),
            Value::Text(t) => Some(leading_int(t)),
            Value::Blob(b) => Some(leading_int(&String::from_utf8_lossy(b))),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Int(i) => i.to_string(),
            Value::Real(r) => format!("{r:?}"),
            Value::Text(t) => t.clone(),
            Value::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        }
    }
}

fn leading_int(t: &str) -> i64 {
    let t = t.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut n: i64 = 0;
    for c in digits.bytes().take_while(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    if neg {
        -n
    } else {
        n
    }
}

pub struct Db {
    file: File,
    page_size: usize,
    usable: usize,
    pages: u64,
    wal: Option<(File, Vec<(u32, u64)>)>,
}

/// One table row: the rowid and the column values in schema order.
pub type Row = (i64, Vec<Value>);

const MAX_DEPTH: usize = 40;

impl Db {
    pub fn open(path: &Path) -> Result<Db, String> {
        let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let mut h = [0u8; 100];
        file.read_exact(&mut h)
            .map_err(|e| format!("header: {e}"))?;
        if &h[..16] != b"SQLite format 3\0" {
            return Err("not an sqlite3 file".into());
        }
        let raw = u16::from_be_bytes([h[16], h[17]]) as usize;
        let page_size = if raw == 1 { 65536 } else { raw };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err("bad page size".into());
        }
        let reserved = h[20] as usize;
        let enc = u32::from_be_bytes([h[56], h[57], h[58], h[59]]);
        if enc > 1 {
            return Err("only UTF-8 databases are supported".into());
        }
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let mut pages = len / page_size as u64;
        let wal = load_wal(path, page_size).map(|(f, map, size)| {
            pages = size as u64;
            (f, map)
        });
        Ok(Db {
            file,
            page_size,
            usable: page_size - reserved,
            pages,
            wal,
        })
    }

    fn page(&mut self, n: u32) -> Result<Vec<u8>, String> {
        if n == 0 || n as u64 > self.pages {
            return Err(format!("page {n} out of range"));
        }
        let mut buf = vec![0u8; self.page_size];
        if let Some((wf, map)) = self.wal.as_mut() {
            if let Ok(i) = map.binary_search_by_key(&n, |e| e.0) {
                wf.seek(SeekFrom::Start(map[i].1))
                    .map_err(|e| e.to_string())?;
                wf.read_exact(&mut buf).map_err(|e| e.to_string())?;
                return Ok(buf);
            }
        }
        self.file
            .seek(SeekFrom::Start((n as u64 - 1) * self.page_size as u64))
            .map_err(|e| e.to_string())?;
        self.file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }

    /// Every row of the rowid table whose b-tree starts at `root`, in rowid order.
    pub fn scan(&mut self, root: u32) -> Result<Vec<Row>, String> {
        let mut out = Vec::new();
        let mut budget = self.pages as usize + 1;
        self.walk(root, 0, &mut out, &mut budget)?;
        Ok(out)
    }

    fn walk(
        &mut self,
        n: u32,
        depth: usize,
        out: &mut Vec<Row>,
        budget: &mut usize,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH || *budget == 0 {
            return Err("b-tree too deep or cyclic".into());
        }
        *budget -= 1;
        let page = self.page(n)?;
        let base = if n == 1 { 100 } else { 0 };
        let kind = *page.get(base).ok_or("short page")?;
        let cells = be16(&page, base + 3)? as usize;
        match kind {
            0x05 => {
                let right = be32(&page, base + 8)?;
                for i in 0..cells {
                    let at = be16(&page, base + 12 + i * 2)? as usize;
                    let child = be32(&page, at)?;
                    self.walk(child, depth + 1, out, budget)?;
                }
                self.walk(right, depth + 1, out, budget)
            }
            0x0d => {
                for i in 0..cells {
                    let at = be16(&page, base + 8 + i * 2)? as usize;
                    let (size, a) = varint(&page, at)?;
                    let (rowid, a) = varint(&page, a)?;
                    let payload = self.payload(&page, a, size as usize)?;
                    out.push((rowid as i64, record(&payload)?));
                }
                Ok(())
            }
            other => Err(format!("page {n}: not a table b-tree page ({other:#x})")),
        }
    }

    fn payload(&mut self, page: &[u8], at: usize, size: usize) -> Result<Vec<u8>, String> {
        let u = self.usable;
        let x = u - 35;
        if size <= x {
            return page
                .get(at..at + size)
                .map(|s| s.to_vec())
                .ok_or_else(|| "cell past page end".to_string());
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((size - m) % (u - 4));
        let local = if k <= x { k } else { m };
        let mut data = page
            .get(at..at + local)
            .ok_or("cell past page end")?
            .to_vec();
        let mut next = be32(page, at + local)?;
        let mut guard = self.pages as usize + 1;
        while data.len() < size {
            if next == 0 || guard == 0 {
                return Err("broken overflow chain".into());
            }
            guard -= 1;
            let ov = self.page(next)?;
            next = be32(&ov, 0)?;
            let take = (size - data.len()).min(u - 4);
            data.extend_from_slice(&ov[4..4 + take]);
        }
        Ok(data)
    }

    /// Number of rows in the b-tree at `root` (rowid table or WITHOUT ROWID
    /// index b-tree), counted from cell counts without decoding records.
    pub fn count(&mut self, root: u32) -> Result<i64, String> {
        let mut budget = self.pages as usize + 1;
        self.count_at(root, 0, &mut budget)
    }

    fn count_at(&mut self, n: u32, depth: usize, budget: &mut usize) -> Result<i64, String> {
        if depth > MAX_DEPTH || *budget == 0 {
            return Err("b-tree too deep or cyclic".into());
        }
        *budget -= 1;
        let page = self.page(n)?;
        let base = if n == 1 { 100 } else { 0 };
        let kind = *page.get(base).ok_or("short page")?;
        let cells = be16(&page, base + 3)? as usize;
        match kind {
            0x0d | 0x0a => Ok(cells as i64),
            0x05 | 0x02 => {
                // interior index cells are rows themselves; table ones are not
                let mut total = if kind == 0x02 { cells as i64 } else { 0 };
                for i in 0..cells {
                    let at = be16(&page, base + 12 + i * 2)? as usize;
                    total += self.count_at(be32(&page, at)?, depth + 1, budget)?;
                }
                Ok(total + self.count_at(be32(&page, base + 8)?, depth + 1, budget)?)
            }
            other => Err(format!("page {n}: not a b-tree page ({other:#x})")),
        }
    }

    /// (name, rootpage) of every ordinary table, sorted by name (BINARY), the
    /// same list as `SELECT name FROM sqlite_master WHERE type='table' AND
    /// name NOT LIKE 'sqlite_%' ORDER BY name`.
    pub fn tables(&mut self) -> Result<Vec<(String, u32)>, String> {
        let mut out: Vec<(String, u32)> = Vec::new();
        for (_, cols) in self.scan(1)? {
            let get = |i: usize| cols.get(i).map(|v| v.text()).unwrap_or_default();
            let name = get(1);
            if get(0) != "table" || name.to_ascii_lowercase().starts_with("sqlite_") {
                continue;
            }
            let root = cols.get(3).and_then(|v| v.cast_int()).unwrap_or(0);
            out.push((name, root.clamp(0, u32::MAX as i64) as u32));
        }
        out.sort();
        Ok(out)
    }

    /// (rootpage, sql) of a table from sqlite_master.
    pub fn table(&mut self, name: &str) -> Result<(u32, String), String> {
        for (_, cols) in self.scan(1)? {
            let get = |i: usize| cols.get(i).map(|v| v.text()).unwrap_or_default();
            if get(0) == "table" && get(1).eq_ignore_ascii_case(name) {
                let root = cols.get(3).and_then(|v| v.cast_int()).unwrap_or(0);
                return Ok((root as u32, get(4)));
            }
        }
        Err(format!("no table {name}"))
    }
}

/// Committed WAL frames: page number -> byte offset of the page image in the
/// -wal file (newest committed frame wins), and the database size in pages
/// after the last commit. Frames past the last valid commit are ignored, as
/// SQLite's own recovery does.
/// (open -wal file, page -> frame offset, db size in pages)
type Wal = (File, Vec<(u32, u64)>, u32);

fn load_wal(path: &Path, page_size: usize) -> Option<Wal> {
    let mut wal_path = path.as_os_str().to_owned();
    wal_path.push("-wal");
    let mut f = File::open(Path::new(&wal_path)).ok()?;
    let mut hdr = [0u8; 32];
    f.read_exact(&mut hdr).ok()?;
    let magic = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let big = match magic {
        0x377f0683 => true,
        0x377f0682 => false,
        _ => return None,
    };
    let wal_page = u32::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    if wal_page != page_size {
        return None;
    }
    let salt = [hdr[16..20].to_vec(), hdr[20..24].to_vec()].concat();
    let (mut s0, mut s1) = wal_sum(&hdr[..24], big, 0, 0);
    if (s0, s1) != (be(&hdr, 24), be(&hdr, 28)) {
        return None;
    }
    let mut pending: Vec<(u32, u64)> = Vec::new();
    let mut committed: Vec<(u32, u64)> = Vec::new();
    let mut size = 0u32;
    let mut off = 32u64;
    let mut fh = [0u8; 24];
    let mut page = vec![0u8; page_size];
    loop {
        if f.read_exact(&mut fh).is_err() || f.read_exact(&mut page).is_err() {
            break;
        }
        if fh[8..16] != salt[..] {
            break;
        }
        let (a, b) = wal_sum(&fh[..8], big, s0, s1);
        let (a, b) = wal_sum(&page, big, a, b);
        if (a, b) != (be(&fh, 16), be(&fh, 20)) {
            break;
        }
        (s0, s1) = (a, b);
        pending.push((be(&fh, 0), off + 24));
        let commit = be(&fh, 4);
        if commit != 0 {
            committed.append(&mut pending);
            size = commit;
        }
        off += 24 + page_size as u64;
    }
    if committed.is_empty() {
        return None;
    }
    // newest frame per page wins
    let mut map: Vec<(u32, u64)> = Vec::new();
    for (pg, at) in committed {
        match map.iter_mut().find(|e| e.0 == pg) {
            Some(e) => e.1 = at,
            None => map.push((pg, at)),
        }
    }
    map.sort_by_key(|e| e.0);
    Some((f, map, size))
}

fn be(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn wal_sum(data: &[u8], big: bool, mut s0: u32, mut s1: u32) -> (u32, u32) {
    for c in data.as_chunks::<8>().0 {
        let w = |s: &[u8]| {
            let a = [s[0], s[1], s[2], s[3]];
            if big {
                u32::from_be_bytes(a)
            } else {
                u32::from_le_bytes(a)
            }
        };
        s0 = s0.wrapping_add(w(&c[..4])).wrapping_add(s1);
        s1 = s1.wrapping_add(w(&c[4..])).wrapping_add(s0);
    }
    (s0, s1)
}

fn be16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_be_bytes([s[0], s[1]]))
        .ok_or_else(|| "short read".to_string())
}

fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "short read".to_string())
}

/// SQLite varint: 1-9 bytes, big-endian, 7 bits each, the 9th byte whole.
pub fn varint(b: &[u8], mut at: usize) -> Result<(u64, usize), String> {
    let mut v: u64 = 0;
    for i in 0..9 {
        let c = *b.get(at).ok_or("short varint")?;
        at += 1;
        if i == 8 {
            return Ok(((v << 8) | c as u64, at));
        }
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return Ok((v, at));
        }
    }
    Ok((v, at))
}

/// Decode one record (header of serial types, then the values).
pub fn record(p: &[u8]) -> Result<Vec<Value>, String> {
    let (hlen, mut h) = varint(p, 0)?;
    let hlen = hlen as usize;
    if hlen > p.len() {
        return Err("record header past payload".into());
    }
    let mut body = hlen;
    let mut out = Vec::new();
    while h < hlen {
        let (t, next) = varint(p, h)?;
        h = next;
        let int = |n: usize, body: usize| -> Result<i64, String> {
            let s = p.get(body..body + n).ok_or("short int")?;
            let mut v: i64 = if s[0] & 0x80 != 0 { -1 } else { 0 };
            for &c in s {
                v = (v << 8) | c as i64;
            }
            Ok(v)
        };
        let (val, used) = match t {
            0 => (Value::Null, 0),
            1 => (Value::Int(int(1, body)?), 1),
            2 => (Value::Int(int(2, body)?), 2),
            3 => (Value::Int(int(3, body)?), 3),
            4 => (Value::Int(int(4, body)?), 4),
            5 => (Value::Int(int(6, body)?), 6),
            6 => (Value::Int(int(8, body)?), 8),
            7 => {
                let s = p.get(body..body + 8).ok_or("short real")?;
                let mut a = [0u8; 8];
                a.copy_from_slice(s);
                (Value::Real(f64::from_be_bytes(a)), 8)
            }
            8 => (Value::Int(0), 0),
            9 => (Value::Int(1), 0),
            10 | 11 => return Err("reserved serial type".into()),
            n => {
                let len = ((n - 12) / 2) as usize;
                let s = p.get(body..body + len).ok_or("short text/blob")?;
                if n % 2 == 0 {
                    (Value::Blob(s.to_vec()), len)
                } else {
                    (Value::Text(String::from_utf8_lossy(s).into_owned()), len)
                }
            }
        };
        body += used;
        out.push(val);
    }
    Ok(out)
}

/// Column names of a CREATE TABLE statement, in order, plus the index of an
/// INTEGER PRIMARY KEY column (stored as NULL; its value is the rowid).
pub fn columns(sql: &str) -> (Vec<String>, Option<usize>) {
    let (Some(open), Some(close)) = (sql.find('('), sql.rfind(')')) else {
        return (Vec::new(), None);
    };
    let inner = &sql[open + 1..close.max(open + 1)];
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    let mut names = Vec::new();
    let mut ipk = None;
    for def in parts {
        let def = def.trim();
        let first = def
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or("");
        let upper = first.to_ascii_uppercase();
        if def.is_empty()
            || ["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"].contains(&upper.as_str())
        {
            continue;
        }
        let name = first.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']' | '\''));
        let rest = def.to_ascii_uppercase();
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.len() >= 4 && words[1] == "INTEGER" && words[2] == "PRIMARY" && words[3] == "KEY" {
            ipk = Some(names.len());
        }
        names.push(name.to_string());
    }
    (names, ipk)
}

/// guardian.db `health_checks`: the newest row (largest id) per service, in id
/// order, as (service, status, CAST(response_time_ms AS INT)). Same result as
/// the old query `... WHERE id IN (SELECT MAX(id) FROM health_checks GROUP BY
/// service)`; rows whose service is NULL or empty are dropped by the caller.
pub fn latest_health(path: &Path) -> Result<Vec<(String, String, i64)>, String> {
    let mut db = Db::open(path)?;
    let (root, sql) = db.table("health_checks")?;
    let (cols, ipk) = columns(&sql);
    let idx = |n: &str| cols.iter().position(|c| c.eq_ignore_ascii_case(n));
    let (Some(si), Some(st), Some(ms)) = (idx("service"), idx("status"), idx("response_time_ms"))
    else {
        return Err("health_checks lacks service/status/response_time_ms".into());
    };
    let id_col = idx("id");
    let mut latest: Vec<(Option<String>, i64, String, i64)> = Vec::new();
    for (rowid, vals) in db.scan(root)? {
        let at = |i: usize| vals.get(i).cloned().unwrap_or(Value::Null);
        let id = match id_col {
            Some(i) if Some(i) != ipk => at(i).cast_int().unwrap_or(rowid),
            _ => rowid,
        };
        let service = match at(si) {
            Value::Null => None,
            v => Some(v.text()),
        };
        let status = at(st).text();
        let ms_v = at(ms).cast_int().unwrap_or(0);
        match latest.iter_mut().find(|r| r.0 == service) {
            Some(r) if id > r.1 => *r = (service, id, status, ms_v),
            Some(_) => {}
            None => latest.push((service, id, status, ms_v)),
        }
    }
    latest.sort_by_key(|r| r.1);
    Ok(latest
        .into_iter()
        .map(|(s, _, st, ms)| (s.unwrap_or_default(), st, ms))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints() {
        assert_eq!(varint(&[0x05], 0).unwrap(), (5, 1));
        assert_eq!(varint(&[0x81, 0x00], 0).unwrap(), (128, 2));
        assert_eq!(varint(&[0xff; 9], 0).unwrap(), (u64::MAX, 9));
    }

    #[test]
    fn record_types() {
        // header 6 bytes: len, NULL, int8, real, text(3), one
        let mut p = vec![6u8, 0, 1, 7, 13 + 6, 9];
        p.push(0xfe); // -2
        p.extend_from_slice(&12.75f64.to_be_bytes());
        p.extend_from_slice(b"abc");
        let v = record(&p).unwrap();
        assert_eq!(
            v,
            vec![
                Value::Null,
                Value::Int(-2),
                Value::Real(12.75),
                Value::Text("abc".into()),
                Value::Int(1)
            ]
        );
        assert_eq!(v[2].cast_int(), Some(12));
        assert_eq!(Value::Real(-3.9).cast_int(), Some(-3));
        assert_eq!(Value::Text(" 42ms".into()).cast_int(), Some(42));
    }

    #[test]
    fn create_table_columns() {
        let sql = "CREATE TABLE health_checks (\n id integer primary key,\n service text,\n status text,\n response_time_ms real, error text, checked_at text, UNIQUE(id, service))";
        let (cols, ipk) = columns(sql);
        assert_eq!(
            cols,
            [
                "id",
                "service",
                "status",
                "response_time_ms",
                "error",
                "checked_at"
            ]
        );
        assert_eq!(ipk, Some(0));
    }

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guardian-fixture.db")
    }

    #[test]
    fn golden_latest_health_on_fixture() {
        let rows = latest_health(&fixture()).expect("fixture db");
        let want = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guardian-fixture.expected"),
        )
        .unwrap();
        let got: String = rows
            .iter()
            .map(|(s, st, ms)| format!("{s} {st} {ms}\n"))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn not_sqlite_is_error() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(latest_health(&p).is_err());
        assert!(latest_health(Path::new("/nonexistent/guardian.db")).is_err());
    }

    #[test]
    fn wal_fixture_counts_match_sqlite() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut db = Db::open(&dir.join("memory-wal-fixture.db")).expect("wal db");
        let mut got = String::new();
        for (name, root) in db.tables().unwrap() {
            got += &format!("{name} {}\n", db.count(root).unwrap());
        }
        let want = std::fs::read_to_string(dir.join("memory-wal-fixture.expected")).unwrap();
        assert_eq!(got, want);
    }

    /// Optional check against a real copy of guardian.db (never the live file):
    /// ONE_TEST_GUARDIAN_DB=<copy> ONE_TEST_GUARDIAN_EXPECTED=<golden from SQLite>.
    #[test]
    fn real_guardian_copy_if_given() {
        let (Ok(db), Ok(want)) = (
            std::env::var("ONE_TEST_GUARDIAN_DB"),
            std::env::var("ONE_TEST_GUARDIAN_EXPECTED"),
        ) else {
            return;
        };
        let got: String = latest_health(Path::new(&db))
            .unwrap()
            .iter()
            .map(|(s, st, ms)| format!("{s} {st} {ms}\n"))
            .collect();
        assert_eq!(got, std::fs::read_to_string(want).unwrap());
    }
}
