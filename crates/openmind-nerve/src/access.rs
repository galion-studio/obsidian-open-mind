// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Access logs and the request-rate anomaly (DDoS) detector.
//!
//! Inputs, all optional:
//! - access-log files (`NERVE_ACCESS_LOGS=name=path,...`): common/combined
//!   log format (nginx, caddy `common_log`, tanga) or one JSON object per
//!   line (`ts|time`, `ip|remote_addr|client_ip`, `path|uri|request`,
//!   `status`). Tailed by offset; rotation is detected by inode or shrink.
//! - the nerve's own API (`nerve-api` stream), recorded per request.
//! - counter-only streams such as cloudflared's `total_requests` delta
//!   (`edge`), which carry rates but no client IPs.
//!
//! Detector (per stream, CPU-light: 10 s buckets, at most 2 h kept, per-IP
//! maps only for the last minute and capped):
//! - `rps` = requests in the last 60 s / 60.
//! - baseline = median and MAD of the 10 s bucket rates from 30 min to 2 min
//!   ago (empty buckets count as 0).
//! - threshold = max(`min_rps`, 5 x median, median + 8 x 1.4826 x MAD).
//! - `flood` when rps >= threshold and the minute holds >= `min_requests`;
//!   `elevated` at half the threshold; else `normal`.
//! - shape: `single-source` when one IP sends >= 50 % of the minute,
//!   `distributed` with >= 50 distinct IPs, else `concentrated`.
//!   Scanner paths and the 4xx/5xx share of the minute are reported too.

use crate::logscan::is_scan_path;
use crate::util::{parse_clf_time, parse_rfc3339, ts};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub const BUCKET_S: u64 = 10;
const KEEP_BUCKETS: usize = 720; // 2 h
const IP_WINDOW_BUCKETS: usize = 6; // 60 s
const MAX_IPS_PER_BUCKET: usize = 512;

/// One request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hit {
    pub t: u64,
    pub ip: String,
    pub path: String,
    pub status: u16,
}

/// Parse one access-log line (combined/common log format or JSON).
/// `now` stands in for a missing or unreadable timestamp.
pub fn parse_line(line: &str, now: u64) -> Option<Hit> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    if line.starts_with('{') {
        let v: Value = serde_json::from_str(line).ok()?;
        let s = |ks: &[&str]| {
            ks.iter()
                .find_map(|k| v.get(*k).and_then(Value::as_str).map(str::to_string))
        };
        let t = ["ts", "time", "timestamp", "@timestamp"]
            .iter()
            .find_map(|k| match v.get(*k) {
                Some(Value::String(s)) => parse_rfc3339(s).or_else(|| parse_clf_time(s)),
                Some(Value::Number(n)) => n.as_f64().map(|f| f as u64),
                _ => None,
            })
            .unwrap_or(now);
        let mut path = s(&["path", "uri", "url", "request"]).unwrap_or_default();
        if let Some((_, p)) = path.split_once(' ') {
            path = p.split(' ').next().unwrap_or("").to_string();
        }
        let status = v
            .get("status")
            .and_then(|x| {
                x.as_u64()
                    .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
            })
            .and_then(|n| u16::try_from(n).ok())
            .unwrap_or(0);
        return Some(Hit {
            t,
            // Capped like the CLF branch (T3.1 fuzz find: a JSON line kept an
            // unbounded client address, the per-IP map key).
            ip: s(&["ip", "remote_addr", "client_ip", "client", "remote_ip"])
                .unwrap_or_default()
                .chars()
                .take(64)
                .collect(),
            path: path.chars().take(200).collect(),
            status,
        });
    }
    // CLF: ip ident user [time] "METHOD path PROTO" status bytes ...
    let ip = line.split_whitespace().next()?.to_string();
    let t = match (line.find('['), line.find(']')) {
        (Some(a), Some(b)) if b > a => parse_clf_time(&line[a + 1..b]).unwrap_or(now),
        _ => now,
    };
    let (mut path, mut status) = (String::new(), 0u16);
    if let Some(q1) = line.find('"') {
        let rest = &line[q1 + 1..];
        if let Some(q2) = rest.find('"') {
            path = rest[..q2]
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            status = rest[q2 + 1..]
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        }
    }
    Some(Hit {
        t,
        ip: ip.chars().take(64).collect(),
        path: path.chars().take(200).collect(),
        status,
    })
}

/// Incremental reader of one log file.
#[derive(Debug)]
pub struct Tail {
    pub name: String,
    pub path: PathBuf,
    offset: u64,
    ino: u64,
    partial: String,
    started: bool,
    from_start: bool,
}

impl Tail {
    /// `from_start`: read the existing content on the first poll (replay);
    /// otherwise start at the end (live: only new requests count).
    pub fn new(name: &str, path: PathBuf, from_start: bool) -> Self {
        Tail {
            name: name.into(),
            path,
            offset: 0,
            ino: 0,
            partial: String::new(),
            started: false,
            from_start,
        }
    }

    /// New complete lines since the last poll (at most 4 MiB per poll).
    pub fn poll(&mut self) -> std::io::Result<Vec<String>> {
        use std::os::unix::fs::MetadataExt;
        let md = std::fs::metadata(&self.path)?;
        let (ino, len) = (md.ino(), md.len());
        if !self.started {
            self.started = true;
            self.ino = ino;
            self.offset = if self.from_start { 0 } else { len };
        }
        if ino != self.ino || len < self.offset {
            self.ino = ino;
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(vec![]);
        }
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        let n = f.take(4 << 20).read_to_end(&mut buf)?;
        self.offset += n as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<String> = self.partial.split('\n').map(str::to_string).collect();
        self.partial = lines.pop().unwrap_or_default();
        if self.partial.len() > 64 * 1024 {
            self.partial.clear();
        }
        Ok(lines)
    }
}

#[derive(Debug, Clone, Default)]
struct Bucket {
    start: u64,
    count: u64,
    errors: u64,
    scans: u64,
    ips: HashMap<String, u32>,
    ip_overflow: u64,
}

/// One request stream (a service's access log, the nerve API, the edge).
#[derive(Debug, Clone, Default)]
pub struct Stream {
    pub name: String,
    /// Component this stream fronts (for incident correlation).
    pub component: Option<String>,
    buckets: VecDeque<Bucket>,
    pub total: u64,
    pub first_seen: u64,
    pub has_ips: bool,
    pub flood_since: Option<u64>,
    pub floods: u32,
    pub peak_rps: f64,
}

impl Stream {
    pub fn new(name: &str, component: Option<String>) -> Self {
        Stream {
            name: name.into(),
            component,
            ..Default::default()
        }
    }

    /// Kept buckets: (count, first bucket start, last bucket start), unix
    /// seconds. Read-only view for tests and the DST replay (slot 157).
    pub fn window(&self) -> (usize, u64, u64) {
        (
            self.buckets.len(),
            self.buckets.front().map_or(0, |b| b.start),
            self.buckets.back().map_or(0, |b| b.start),
        )
    }

    fn bucket(&mut self, t: u64) -> Option<&mut Bucket> {
        let start = t - t % BUCKET_S;
        if self.first_seen == 0 || start < self.first_seen {
            self.first_seen = start;
        }
        if let Some(i) = self.buckets.iter().rposition(|b| b.start == start) {
            return self.buckets.get_mut(i);
        }
        if self.buckets.back().is_some_and(|b| b.start > start) {
            // Older than the newest bucket and not present: insert in order.
            let i = self.buckets.partition_point(|b| b.start < start);
            if self.buckets.len() >= KEEP_BUCKETS && i == 0 {
                return None;
            }
            self.buckets.insert(
                i,
                Bucket {
                    start,
                    ..Default::default()
                },
            );
            while self.buckets.len() > KEEP_BUCKETS {
                self.buckets.pop_front();
            }
            return self.buckets.get_mut(i);
        }
        self.buckets.push_back(Bucket {
            start,
            ..Default::default()
        });
        while self.buckets.len() > KEEP_BUCKETS {
            self.buckets.pop_front();
        }
        // Per-IP maps only matter for the last minute.
        let n = self.buckets.len();
        for b in self
            .buckets
            .iter_mut()
            .take(n.saturating_sub(IP_WINDOW_BUCKETS + 1))
        {
            if !b.ips.is_empty() {
                b.ips = HashMap::new();
            }
        }
        self.buckets.back_mut()
    }

    pub fn record(&mut self, h: &Hit) {
        let has_ip = !h.ip.is_empty();
        let Some(b) = self.bucket(h.t) else { return };
        b.count += 1;
        if h.status >= 400 {
            b.errors += 1;
        }
        if is_scan_path(&h.path) {
            b.scans += 1;
        }
        if has_ip {
            if b.ips.len() < MAX_IPS_PER_BUCKET || b.ips.contains_key(&h.ip) {
                *b.ips.entry(h.ip.clone()).or_insert(0) += 1;
            } else {
                b.ip_overflow += 1;
            }
        }
        self.total += 1;
        if has_ip {
            self.has_ips = true;
        }
    }

    /// Counter-only input: `n` requests at time `t` (no IPs).
    pub fn record_count(&mut self, t: u64, n: u64, errors: u64) {
        if let Some(b) = self.bucket(t) {
            b.count += n;
            b.errors += errors;
        }
        self.total += n;
    }

    /// Evaluate at `now`.
    pub fn evaluate(&mut self, now: u64, p: &DdosParams) -> DdosVerdict {
        let win_from = now.saturating_sub(60);
        let (mut cnt, mut errs, mut scans, mut ovf) = (0u64, 0u64, 0u64, 0u64);
        let mut ips: HashMap<&str, u64> = HashMap::new();
        for b in self
            .buckets
            .iter()
            .filter(|b| b.start + BUCKET_S > win_from && b.start <= now)
        {
            cnt += b.count;
            errs += b.errors;
            scans += b.scans;
            ovf += b.ip_overflow;
            for (ip, c) in &b.ips {
                *ips.entry(ip.as_str()).or_insert(0) += *c as u64;
            }
        }
        let rps = cnt as f64 / 60.0;
        // Baseline: 10 s buckets in [now-1800, now-120), zeros included,
        // but never before the stream was first seen.
        let base_to = now.saturating_sub(120);
        let base_from = now.saturating_sub(1800).max(self.first_seen);
        let mut rates = vec![];
        let mut t = base_from - base_from % BUCKET_S;
        let by_start: BTreeMap<u64, u64> = self
            .buckets
            .iter()
            .filter(|b| b.start >= t && b.start < base_to)
            .map(|b| (b.start, b.count))
            .collect();
        while t + BUCKET_S <= base_to {
            rates.push(*by_start.get(&t).unwrap_or(&0) as f64 / BUCKET_S as f64);
            t += BUCKET_S;
        }
        let (median, mad) = if rates.len() >= 6 {
            let m = crate::util::median(&rates).unwrap_or(0.0);
            let dev: Vec<f64> = rates.iter().map(|r| (r - m).abs()).collect();
            (m, crate::util::median(&dev).unwrap_or(0.0))
        } else {
            (0.0, 0.0)
        };
        let threshold = p.min_rps.max(median * 5.0).max(median + 8.0 * 1.4826 * mad);
        let level = if rps >= threshold && cnt >= p.min_requests {
            "flood"
        } else if rps >= threshold / 2.0 && cnt >= p.min_requests / 2 {
            "elevated"
        } else {
            "normal"
        };
        let mut top: Vec<(&str, u64)> = ips.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let distinct = top.len() as u64 + u64::from(ovf > 0);
        let top_share = top
            .first()
            .map(|x| x.1 as f64 / cnt.max(1) as f64)
            .unwrap_or(0.0);
        let shape = if !self.has_ips {
            "unknown (counter only)"
        } else if top_share >= 0.5 {
            "single-source"
        } else if distinct >= 50 {
            "distributed"
        } else {
            "concentrated"
        };
        if level == "flood" {
            if self.flood_since.is_none() {
                self.flood_since = Some(now);
                self.floods += 1;
            }
        } else if level == "normal" {
            self.flood_since = None;
        }
        self.peak_rps = self.peak_rps.max(rps);
        DdosVerdict {
            stream: self.name.clone(),
            component: self.component.clone(),
            level,
            rps,
            requests_60s: cnt,
            baseline_rps: median,
            mad_rps: mad,
            baseline_buckets: rates.len(),
            threshold_rps: threshold,
            factor: if median > 0.0 { rps / median } else { 0.0 },
            shape,
            top_ips: top
                .iter()
                .take(5)
                .map(|(i, c)| (i.to_string(), *c))
                .collect(),
            distinct_ips: distinct,
            error_share: errs as f64 / cnt.max(1) as f64,
            scan_hits_60s: scans,
            flood_since: self.flood_since,
            floods_total: self.floods,
            peak_rps: self.peak_rps,
            total: self.total,
        }
    }
}

/// Detector knobs (env: NERVE_DDOS_MIN_RPS, NERVE_DDOS_MIN_REQUESTS).
#[derive(Debug, Clone)]
pub struct DdosParams {
    pub min_rps: f64,
    pub min_requests: u64,
}

impl Default for DdosParams {
    fn default() -> Self {
        DdosParams {
            min_rps: 10.0,
            min_requests: 300,
        }
    }
}

/// Result of one evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct DdosVerdict {
    pub stream: String,
    pub component: Option<String>,
    pub level: &'static str,
    pub rps: f64,
    pub requests_60s: u64,
    pub baseline_rps: f64,
    pub mad_rps: f64,
    pub baseline_buckets: usize,
    pub threshold_rps: f64,
    pub factor: f64,
    pub shape: &'static str,
    pub top_ips: Vec<(String, u64)>,
    pub distinct_ips: u64,
    pub error_share: f64,
    pub scan_hits_60s: u64,
    pub flood_since: Option<u64>,
    pub floods_total: u32,
    pub peak_rps: f64,
    pub total: u64,
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

impl DdosVerdict {
    pub fn explain(&self) -> String {
        let src = match self.shape {
            "single-source" => format!(
                "single source {} ({} of {} requests)",
                self.top_ips.first().map(|x| x.0.as_str()).unwrap_or("?"),
                self.top_ips.first().map(|x| x.1).unwrap_or(0),
                self.requests_60s
            ),
            "distributed" => format!("distributed over {} IPs", self.distinct_ips),
            s => format!("{s}, {} IPs", self.distinct_ips),
        };
        format!(
            "{}: {:.1} req/s over the last 60 s vs baseline {:.2} req/s (threshold {:.1}, x{:.0}); {src}; {:.0}% 4xx/5xx",
            self.stream,
            self.rps,
            self.baseline_rps,
            self.threshold_rps,
            self.factor,
            self.error_share * 100.0
        )
    }

    pub fn to_json(&self) -> Value {
        json!({
            "stream": self.stream, "component": self.component, "level": self.level,
            "rps": r2(self.rps), "requests_60s": self.requests_60s, "baseline_rps": r2(self.baseline_rps),
            "mad_rps": r2(self.mad_rps), "baseline_buckets": self.baseline_buckets,
            "threshold_rps": r2(self.threshold_rps), "factor": r2(self.factor), "shape": self.shape,
            "top_ips": self.top_ips.iter().map(|(i, c)| json!({"ip": i, "requests": c})).collect::<Vec<_>>(),
            "distinct_ips": self.distinct_ips, "error_share": r2(self.error_share),
            "scan_hits_60s": self.scan_hits_60s,
            "flood_since": self.flood_since.map(ts), "floods_total": self.floods_total,
            "peak_rps": r2(self.peak_rps), "total": self.total,
            "explain": self.explain(),
        })
    }
}

/// All streams.
#[derive(Debug, Clone, Default)]
pub struct Detector {
    pub streams: BTreeMap<String, Stream>,
    pub params: DdosParams,
}

impl Detector {
    pub fn stream(&mut self, name: &str, component: Option<&str>) -> &mut Stream {
        self.streams
            .entry(name.to_string())
            .or_insert_with(|| Stream::new(name, component.map(str::to_string)))
    }

    pub fn evaluate(&mut self, now: u64) -> Vec<DdosVerdict> {
        let p = self.params.clone();
        self.streams
            .values_mut()
            .map(|s| s.evaluate(now, &p))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clf_and_json() {
        let h = parse_line(
            r#"203.0.113.9 - - [20/Oct/2026:19:04:34 +0200] "GET /.env HTTP/1.1" 404 12 "-" "curl""#,
            1,
        )
        .unwrap();
        assert_eq!(h.ip, "203.0.113.9");
        assert_eq!(h.path, "/.env");
        assert_eq!(h.status, 404);
        assert_eq!(ts(h.t), "2026-10-20T19:04:34+02:00");
        let j = parse_line(
            r#"{"ts":"2026-10-20T19:04:34+02:00","ip":"::1","path":"/state","status":200}"#,
            1,
        )
        .unwrap();
        assert_eq!(
            (j.ip.as_str(), j.path.as_str(), j.status),
            ("::1", "/state", 200)
        );
        let j = parse_line(r#"{"request":"GET /x HTTP/1.1","status":"503"}"#, 7).unwrap();
        assert_eq!((j.t, j.path.as_str(), j.status), (7, "/x", 503));
        assert!(parse_line("", 0).is_none());
    }

    fn hit(t: u64, ip: &str) -> Hit {
        Hit {
            t,
            ip: ip.into(),
            path: "/".into(),
            status: 200,
        }
    }

    #[test]
    fn quiet_baseline_then_flood_fires() {
        let mut s = Stream::new("tuner", Some("tuner".into()));
        let p = DdosParams::default();
        let t0 = 1_000_000;
        // 20 minutes of ~1 req / 5 s from a few IPs.
        for i in 0..240u64 {
            s.record(&hit(t0 + i * 5, ["a", "b", "c"][(i % 3) as usize]));
        }
        let now = t0 + 1200;
        let v = s.evaluate(now, &p);
        assert_eq!(v.level, "normal", "{}", v.explain());
        assert!((v.baseline_rps - 0.2).abs() < 0.11, "{}", v.baseline_rps);
        // 60 s at 50 req/s from one IP.
        for i in 0..3000u64 {
            s.record(&hit(now + i / 50, "198.51.100.7"));
        }
        let v = s.evaluate(now + 60, &p);
        assert_eq!(v.level, "flood", "{}", v.explain());
        assert_eq!(v.shape, "single-source");
        assert_eq!(v.top_ips[0].0, "198.51.100.7");
        assert!(v.rps > 40.0);
        assert!(v.explain().contains("single source 198.51.100.7"));
        assert_eq!(v.floods_total, 1);
        // Quiet again: back to normal after the window.
        let v = s.evaluate(now + 200, &p);
        assert_eq!(v.level, "normal");
        assert!(v.flood_since.is_none());
    }

    #[test]
    fn busy_baseline_is_not_a_flood() {
        let mut s = Stream::new("edge", None);
        let p = DdosParams::default();
        let t0 = 2_000_000;
        // Steady 20 req/s for 30 min (counter input).
        for i in 0..180u64 {
            s.record_count(t0 + i * 10, 200, 0);
        }
        let v = s.evaluate(t0 + 1800, &p);
        assert_eq!(v.level, "normal", "{}", v.explain());
        assert!(v.threshold_rps >= 100.0);
        assert_eq!(v.shape, "unknown (counter only)");
    }

    #[test]
    fn distributed_shape() {
        let mut s = Stream::new("x", None);
        let p = DdosParams::default();
        for i in 0..1200u64 {
            s.record(&hit(
                5_000_000 + i / 20,
                &format!("10.0.{}.{}", i % 7, i % 97),
            ));
        }
        let v = s.evaluate(5_000_060, &p);
        assert_eq!(v.level, "flood");
        assert_eq!(v.shape, "distributed");
    }

    #[test]
    fn tail_reads_new_lines_and_rotation() {
        let dir = std::env::temp_dir().join(format!("nerve-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.log");
        std::fs::write(&p, "old\n").unwrap();
        let mut t = Tail::new("a", p.clone(), false);
        assert!(t.poll().unwrap().is_empty());
        std::fs::write(&p, "old\nnew1\nnew").unwrap();
        assert_eq!(t.poll().unwrap(), vec!["new1"]);
        std::fs::write(&p, "x\n").unwrap();
        assert_eq!(t.poll().unwrap(), vec!["x"]);
        let mut r = Tail::new("a", p.clone(), true);
        assert_eq!(r.poll().unwrap(), vec!["x"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
