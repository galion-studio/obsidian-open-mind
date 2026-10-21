// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Mutation/property fuzz targets for openmind-queue (slot 221).
//!
//! Targets (each a `#[test]`, short fixed-seed pass by default; see
//! `tests/fuzz/mod.rs` for `GALION_FUZZ_SECS` / `GALION_FUZZ_SEED`):
//!
//! - `fuzz_log_line`: `store::decode` on mutated `queue.log` lines (raw, and
//!   with the CRC recomputed so the JSON parser is reached); a decoded
//!   record must re-encode and decode to itself.
//! - `fuzz_store_open`: whole mutated `queue.log` files through
//!   `Store::open`, against an independent oracle of the documented rule
//!   (torn tail cut, a bad line with good lines after it refuses start).
//! - `fuzz_replay_hostile`: CRC-valid but hostile records (duplicate ids,
//!   edge numbers, unknown ids) through `Engine::apply`, then every read
//!   the server does (`depth`, `summary`, `lease`, `expire_due`,
//!   `snapshot`). The module promises "a replay is always safe".
//! - `fuzz_job_body`: mutated `POST /jobs` JSON bodies parsed as `JobSpec`
//!   and driven through the decide/apply loop (lease, ack, nack, extend,
//!   expire, redrive) with caps; the spend hard stop, depth accounting and
//!   compaction fidelity are checked after every step.
//! - `fuzz_sidecar`: mutated `refusals.dat` files; opening never fails or
//!   panics and the next save is always what the next open restores.
//! - `fuzz_http_request`: `http::read_request` on mutated requests plus
//!   `parse_response`, `split_url` and `Request::param`.

mod fuzz;

use fuzz::{mutate, run, Rng};
use openmind_queue::engine::{Config, Engine, Job, JobSpec, JobState, Record, Stats, GLOBAL};
use openmind_queue::http;
use openmind_queue::refusals::{Refusals, Sidecar, SLOT};
use openmind_queue::store::{crc32, decode, encode, Store};
use openmind_queue::vram::{Device, DeviceKind, DeviceState};
use std::fs;
use std::path::PathBuf;

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("q221-fuzz-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

const JSON_TOKENS: &[&[u8]] = &[
    b"\"t\":\"enqueue\"",
    b"\"t\":\"lease\"",
    b"\"t\":\"ack\"",
    b"\"t\":\"fail\"",
    b"\"t\":\"snapshot\"",
    b"\"t\":\"counters\"",
    b"\"t\":\"device\"",
    b"\"id\":18446744073709551615",
    b"\"token\":18446744073709551615",
    b"\"reserved\":18446744073709551615",
    b"\"cost\":18446744073709551615",
    b"\"vram_gb\":4294967295",
    b"\"priority\":-2147483648",
    b"\"max_attempts\":4294967295",
    b"\"retry_at\":null",
    b"\"cap\":0",
    b"\"state\":\"leased\"",
    b"\"device\":\"cpu\"",
    b"\"agent\":\"*\"",
    b"\\u0000",
    b"\\ud800",
    b"1e999",
    b"-0",
    b"[",
    b"]",
    b",",
    b"null",
    b"{}",
];

fn spec(r: &mut Rng) -> JobSpec {
    JobSpec {
        queue: r.pick(&["default", "q", "gpu"]).to_string(),
        agent: r.pick(&["a", "b", "anon", "*"]).to_string(),
        kind: r.pick(&["echo", "job", "llm"]).to_string(),
        priority: match r.below(4) {
            0 => i32::MAX,
            1 => i32::MIN,
            _ => r.below(10) as i32 - 5,
        },
        max_attempts: r.below(4) as u32,
        cost: if r.chance(3) { r.edgy_u64() } else { r.below(5) as u64 },
        lease_ms: if r.chance(4) { r.edgy_u64() } else { 1000 },
        payload: "p".repeat(r.below(8)),
        vram_gb: if r.chance(3) { r.edgy_u32() } else { r.below(3) as u32 * 8 },
        cpu_fallback: r.chance(2),
    }
}

fn device(r: &mut Rng) -> Device {
    Device {
        id: r.pick(&["g1", "g2", "lan1", "cpu"]).to_string(),
        kind: *r.pick(&[DeviceKind::Gpu, DeviceKind::Lan, DeviceKind::Remote]),
        vram_gb: if r.chance(3) { r.edgy_u32() } else { 24 },
        provider: String::new(),
        cents_per_hour: if r.chance(3) { r.edgy_u64() } else { 50 },
        state: *r.pick(&[DeviceState::Approved, DeviceState::Planned, DeviceState::Rejected]),
        healthy: r.chance(2),
    }
}

/// A realistic record stream: a small run of the real decide/apply loop.
fn honest_records(r: &mut Rng) -> Vec<Record> {
    let mut e = Engine::new(Config::default());
    let mut out = Vec::new();
    let push = |e: &mut Engine, rec: Record, out: &mut Vec<Record>| {
        e.apply(&rec);
        out.push(rec);
    };
    push(&mut e, Record::Cap { scope: GLOBAL.into(), cap: Some(1000) }, &mut out);
    push(&mut e, Record::Quota { agent: "a".into(), max_pending: 9, max_inflight: 3 }, &mut out);
    push(&mut e, Record::Device(Device {
        id: "g1".into(),
        kind: DeviceKind::Gpu,
        vram_gb: 24,
        provider: String::new(),
        cents_per_hour: 40,
        state: DeviceState::Approved,
        healthy: true,
    }), &mut out);
    let mut now = 1_000_000u64;
    for i in 0..(3 + r.below(6)) {
        let s = JobSpec {
            queue: "q".into(),
            agent: if i % 2 == 0 { "a".into() } else { "b".into() },
            priority: (i % 3) as i32,
            cost: (i % 4) as u64,
            vram_gb: if i % 3 == 0 { 8 } else { 0 },
            cpu_fallback: true,
            payload: format!("job {i}"),
            ..JobSpec::default()
        };
        if let Ok(rec) = e.enqueue(s, now) {
            push(&mut e, rec, &mut out);
        }
    }
    for _ in 0..4 {
        now += 10;
        if let Ok(Record::Lease { id, token, .. }) = e.lease("q", "w", now, 50).map(|x| {
            out.push(x.clone());
            e.apply(&x);
            x
        }) {
            let rec = match r.below(3) {
                0 => e.ack(id, token, Some(1)),
                1 => e.nack(id, token, None, "boom", now),
                _ => e.extend(id, token, now, 500),
            };
            if let Ok(rec) = rec {
                push(&mut e, rec, &mut out);
            }
        }
    }
    for rec in e.expire_due(now + 1_000_000) {
        push(&mut e, rec, &mut out);
    }
    // a real log is either the record stream or (after compaction) the
    // snapshot, never both: both would replay every job twice
    if r.chance(3) {
        e.snapshot()
    } else {
        out
    }
}

/// Re-frame a (possibly mutated) JSON body as a log line with a fresh CRC.
fn line_with_crc(json: &[u8]) -> Vec<u8> {
    let mut v = format!("{:08x} ", crc32(json)).into_bytes();
    v.extend_from_slice(json);
    v
}

#[test]
fn fuzz_log_line() {
    run("log_line", 0x221_0001, 4000, |r, last| {
        let recs = honest_records(r);
        let rec = r.pick(&recs).clone();
        let line = encode(&rec);
        let line = line.trim_end_matches('\n');
        let (_, json) = line.split_once(' ').unwrap();
        let cand = if r.chance(2) {
            line_with_crc(&mutate(r, json.as_bytes(), JSON_TOKENS))
        } else {
            mutate(r, line.as_bytes(), JSON_TOKENS)
        };
        last.extend_from_slice(&cand);
        let Ok(s) = std::str::from_utf8(&cand) else {
            return Ok(());
        };
        if let Some(d) = decode(s) {
            let again = encode(&d);
            let back = decode(again.trim_end_matches('\n'));
            if back.as_ref() != Some(&d) {
                return Err(format!("re-encode did not round-trip: {d:?} -> {back:?}"));
            }
        }
        Ok(())
    });
}

/// Independent oracle of `Store::open`: (Ok(records, good_end) | Err).
fn oracle_open(buf: &[u8]) -> Result<(Vec<Record>, usize), ()> {
    let mut recs = Vec::new();
    let mut good_end = 0usize;
    let mut seen_bad = false;
    let mut pos = 0usize;
    while pos < buf.len() {
        let nl = buf[pos..].iter().position(|&b| b == b'\n');
        let end = nl.map_or(buf.len(), |i| pos + i);
        let rec = nl
            .and_then(|_| std::str::from_utf8(&buf[pos..end]).ok())
            .and_then(decode);
        match rec {
            Some(r) if !seen_bad => {
                recs.push(r);
                good_end = end + 1;
            }
            Some(_) => return Err(()),
            None => seen_bad = true,
        }
        pos = end + 1;
    }
    Ok((recs, good_end))
}

#[test]
fn fuzz_store_open() {
    let dir = tmpdir("store");
    let log = Store::log_path(&dir);
    run("store_open", 0x221_0002, 600, |r, last| {
        let recs = honest_records(r);
        let mut buf: Vec<u8> = recs.iter().flat_map(|x| encode(x).into_bytes()).collect();
        // mutate one line with a fresh crc, or the raw file
        if r.chance(2) {
            let text = String::from_utf8(buf.clone()).unwrap();
            let lines: Vec<&str> = text.split_inclusive('\n').collect();
            let k = r.below(lines.len());
            let mut out = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                if i == k {
                    let body = l.trim_end_matches('\n').split_once(' ').unwrap().1;
                    out.extend(line_with_crc(&mutate(r, body.as_bytes(), JSON_TOKENS)));
                    if r.chance(4) {
                        continue; // lose the newline too: a torn tail
                    }
                    out.push(b'\n');
                } else {
                    out.extend_from_slice(l.as_bytes());
                }
            }
            buf = out;
        } else {
            buf = mutate(r, &buf, &[b"\n", b"00000000 ", b"{}\n"]);
        }
        last.extend_from_slice(&buf);
        fs::write(&log, &buf).map_err(|e| e.to_string())?;
        let want = oracle_open(&buf);
        let got = Store::open(&dir);
        match (want, got) {
            (Err(()), Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => Ok(()),
            (Ok((wr, end)), Ok((mut st, gr, rep))) => {
                if wr != gr {
                    return Err(format!("records differ: oracle {} store {}", wr.len(), gr.len()));
                }
                if rep.truncated_bytes != (buf.len() - end) as u64 {
                    return Err(format!("truncated {} want {}", rep.truncated_bytes, buf.len() - end));
                }
                let len = fs::metadata(&log).map_err(|e| e.to_string())?.len();
                if len != end as u64 {
                    return Err(format!("file len {len} after open, want {end}"));
                }
                // the store stays usable: one more record survives a reopen
                let extra = Record::Next { id: 7, token: 7 };
                st.append(std::slice::from_ref(&extra)).map_err(|e| e.to_string())?;
                drop(st);
                let (_s, again, rep2) = Store::open(&dir).map_err(|e| e.to_string())?;
                if again.len() != gr.len() + 1 || rep2.truncated_bytes != 0 {
                    return Err("append after a cut tail did not survive reopen".into());
                }
                // the server replays every record; that must not panic
                let mut e = Engine::new(Config::default());
                for x in &again {
                    e.apply(x);
                }
                let _ = e.depth();
                Ok(())
            }
            (w, g) => Err(format!(
                "oracle {:?} vs store {:?}",
                w.map(|x| x.0.len()),
                g.map(|x| x.1.len()).map_err(|e| e.to_string())
            )),
        }
    });
    let _ = fs::remove_dir_all(&dir);
}

fn hostile_record(r: &mut Rng) -> Record {
    let id = if r.chance(5) { r.edgy_u64() } else { 1 + r.below(4) as u64 };
    let token = if r.chance(5) { r.edgy_u64() } else { r.below(5) as u64 };
    match r.below(14) {
        0..=2 => Record::Enqueue { id, now: r.edgy_u64(), spec: spec(r) },
        3 | 4 => Record::Lease {
            id,
            token,
            worker: "w".into(),
            deadline: r.edgy_u64(),
            reserved: r.edgy_u64(),
            device: r.pick(&["", "cpu", "g1", "zz"]).to_string(),
        },
        5 => Record::Extend { id, token, deadline: r.edgy_u64() },
        6 => Record::Ack { id, token, charge: r.edgy_u64() },
        7 => Record::Fail {
            id,
            token,
            charge: r.edgy_u64(),
            retry_at: if r.chance(2) { None } else { Some(r.edgy_u64()) },
            expired: r.chance(2),
            error: "e".into(),
        },
        8 => Record::Redrive { id, now: r.edgy_u64() },
        9 => Record::Cap {
            scope: r.pick(&["*", "a", "b"]).to_string(),
            cap: if r.chance(2) { None } else { Some(r.edgy_u64()) },
        },
        10 => Record::Quota {
            agent: r.pick(&["*", "a", "b"]).to_string(),
            max_pending: r.edgy_u32(),
            max_inflight: r.edgy_u32(),
        },
        11 => Record::Device(device(r)),
        12 => {
            let s = spec(r);
            Record::Snapshot(Box::new(Job {
                id,
                spec: s,
                created_ms: 0,
                state: *r.pick(&[JobState::Ready, JobState::Delayed, JobState::Leased, JobState::Dead]),
                attempts: r.edgy_u32(),
                ready_at: r.edgy_u64(),
                token,
                worker: "w".into(),
                deadline: r.edgy_u64(),
                reserved: r.edgy_u64(),
                last_error: String::new(),
                device: r.pick(&["", "cpu", "g1"]).to_string(),
            }))
        }
        _ => match r.below(3) {
            0 => Record::Spent { scope: r.pick(&["*", "a"]).to_string(), spent: r.edgy_u64() },
            1 => Record::Next { id: r.edgy_u64(), token: r.edgy_u64() },
            _ => Record::Counters(Stats {
                enqueued: r.edgy_u64(),
                stale_acks: r.edgy_u64(),
                ..Stats::default()
            }),
        },
    }
}

/// Every read the server's handlers do on an engine, plus some decides.
fn exercise(e: &mut Engine, r: &mut Rng) {
    let _ = e.depth();
    let _ = e.vram.summary();
    let _ = e.dead_jobs();
    let _ = e.next_wake();
    let names: Vec<String> = e.agents().keys().cloned().collect();
    for n in &names {
        let _ = e.quota_of(n);
        let _ = e.agents()[n].budget.remaining();
    }
    let _ = e.global().remaining();
    for q in ["q", "default", "gpu"] {
        let _ = e.lease(q, "w", r.edgy_u64(), r.edgy_u64());
    }
    let _ = e.expire_due(r.edgy_u64());
    let _ = e.enqueue(spec(r), r.edgy_u64());
    let _ = openmind_queue::gaia::load(e);
}

#[test]
fn fuzz_replay_hostile() {
    run("replay_hostile", 0x221_0003, 3000, |r, last| {
        let n = 1 + r.below(24);
        let recs: Vec<Record> = (0..n).map(|_| hostile_record(r)).collect();
        for x in &recs {
            last.extend_from_slice(encode(x).as_bytes());
        }
        let mut e = Engine::new(Config::default());
        for x in &recs {
            e.apply(x);
        }
        exercise(&mut e, r);
        // compaction of a replayed state, replayed again, is a fixed point
        let snap = e.snapshot();
        let mut e2 = Engine::new(Config::default());
        for x in &snap {
            e2.apply(x);
        }
        let _ = e2.depth();
        let snap2 = e2.snapshot();
        if snap != snap2 {
            return Err("snapshot of a replayed snapshot differs".into());
        }
        Ok(())
    });
}

const BODY_CORPUS: &[&str] = &[
    r#"{"kind":"echo","priority":5}"#,
    r#"{"queue":"q","agent":"a","kind":"llm","cost":3,"lease_ms":200,"payload":"hi"}"#,
    r#"{"queue":"gpu","agent":"b","kind":"job","vram_gb":8,"cpu_fallback":true,"max_attempts":2}"#,
    r#"{"agent":"a","cost":18446744073709551615}"#,
    r#"{"priority":-2147483648,"lease_ms":18446744073709551615}"#,
    r#"{}"#,
];

fn check_engine(e: &Engine, caps: &[(String, u64)]) -> Result<(), String> {
    let (ready, delayed, leased, dead) = e.depth();
    if ready + delayed + leased + dead != e.live_jobs() {
        return Err(format!("depth {ready}+{delayed}+{leased}+{dead} != jobs {}", e.live_jobs()));
    }
    for (scope, cap) in caps {
        let b = if scope == GLOBAL { e.global().clone() } else {
            e.agents().get(scope).map(|a| a.budget.clone()).unwrap_or_default()
        };
        if b.spent.checked_add(b.reserved).is_none_or(|t| t > *cap) {
            return Err(format!(
                "hard stop crossed for {scope}: spent {} + reserved {} > cap {cap}",
                b.spent, b.reserved
            ));
        }
    }
    let mut reserved = 0u64;
    let mut inflight: std::collections::BTreeMap<&str, u32> = Default::default();
    for a in e.agents().values() {
        reserved = reserved.checked_add(a.budget.reserved).ok_or("agent reserved sum overflows")?;
    }
    for (n, a) in e.agents() {
        inflight.insert(n.as_str(), a.inflight);
    }
    if reserved != e.global().reserved {
        return Err(format!("sum of agent reservations {reserved} != global {}", e.global().reserved));
    }
    let snap = e.snapshot();
    let mut e2 = Engine::new(Config::default());
    for x in &snap {
        e2.apply(x);
    }
    if e2.snapshot() != snap {
        return Err("compaction is not a fixed point".into());
    }
    if e2.global() != e.global() {
        return Err(format!("global budget after compaction {:?} != {:?}", e2.global(), e.global()));
    }
    for (n, a) in e.agents() {
        let b = e2.agents().get(n).cloned().unwrap_or_default();
        if (a.pending, a.inflight, &a.budget) != (b.pending, b.inflight, &b.budget) {
            return Err(format!("agent {n} after compaction differs"));
        }
    }
    Ok(())
}

#[test]
fn fuzz_job_body() {
    run("job_body", 0x221_0004, 1500, |r, last| {
        let mut e = Engine::new(Config {
            max_depth: 64,
            backoff_base_ms: 5,
            backoff_max_ms: 50,
            ..Config::default()
        });
        let mut caps = Vec::new();
        if r.chance(2) {
            let c = if r.chance(3) { r.edgy_u64() } else { 10 + r.below(20) as u64 };
            let rec = Record::Cap { scope: GLOBAL.into(), cap: Some(c) };
            e.apply(&rec);
            caps.push((GLOBAL.to_string(), c));
        }
        if r.chance(2) {
            let c = r.below(15) as u64;
            e.apply(&Record::Cap { scope: "a".into(), cap: Some(c) });
            caps.push(("a".to_string(), c));
        }
        if r.chance(2) {
            let mut d = device(r);
            if d.validate().is_ok() {
                e.apply(&Record::Device(d.clone()));
                d.id = "g2".into();
                e.apply(&Record::Device(d));
            }
        }
        let mut now = 1_000_000u64;
        let mut leases: Vec<(u64, u64)> = Vec::new();
        for _ in 0..(4 + r.below(28)) {
            now = now.saturating_add(r.below(40) as u64);
            match r.below(8) {
                0..=2 => {
                    let seed_body = *r.pick(BODY_CORPUS);
                    let body = mutate(r, seed_body.as_bytes(), JSON_TOKENS);
                    last.extend_from_slice(&body);
                    last.push(b'\n');
                    let b: &[u8] = if body.is_empty() { b"{}" } else { &body };
                    if let Ok(s) = serde_json::from_slice::<JobSpec>(b) {
                        if let Ok(rec) = e.enqueue(s, now) {
                            e.apply(&rec);
                        }
                    }
                }
                3 => {
                    if let Ok(rec) = e.lease(r.pick(&["default", "q", "gpu"]), "w", now, r.edgy_u64() % 1000) {
                        if let Record::Lease { id, token, .. } = &rec {
                            leases.push((*id, *token));
                        }
                        e.apply(&rec);
                    }
                }
                4 if !leases.is_empty() => {
                    let (id, tok) = leases.swap_remove(r.below(leases.len()));
                    let actual = if r.chance(2) { None } else { Some(r.edgy_u64()) };
                    if let Ok(rec) = e.ack(id, tok, actual) {
                        e.apply(&rec);
                    }
                }
                5 if !leases.is_empty() => {
                    let (id, tok) = leases.swap_remove(r.below(leases.len()));
                    if let Ok(rec) = e.nack(id, tok, Some(r.edgy_u64()), "x", now) {
                        e.apply(&rec);
                    }
                }
                6 => {
                    for rec in e.expire_due(now.saturating_add(r.edgy_u64() % 2000)) {
                        e.apply(&rec);
                    }
                    for j in e.dead_jobs().iter().map(|j| j.id).collect::<Vec<_>>() {
                        if let Ok(rec) = e.redrive(j, now) {
                            e.apply(&rec);
                        }
                    }
                }
                _ => {
                    if let Some(&(id, tok)) = leases.first() {
                        if let Ok(rec) = e.extend(id, tok, now, r.edgy_u64()) {
                            e.apply(&rec);
                        }
                    }
                }
            }
            let _ = e.vram.summary();
            check_engine(&e, &caps)?;
        }
        Ok(())
    });
}

fn refusals(r: &mut Rng) -> Refusals {
    Refusals {
        quota_refusals: r.edgy_u64(),
        budget_refusals: r.below(9) as u64,
        backpressure_refusals: r.edgy_u64(),
        stale_acks: r.below(3) as u64,
        charge_clamped: r.edgy_u64(),
    }
}

/// A slot body the real encoder would write, for crafted files.
fn slot_bytes(seq: u64, c: &Refusals) -> Vec<u8> {
    let json = format!(
        "{{\"seq\":{seq},\"c\":{}}}",
        serde_json::to_string(c).unwrap()
    );
    let mut v = format!("{:08x} {json}\n", crc32(json.as_bytes())).into_bytes();
    v.resize(SLOT, 0);
    v
}

#[test]
fn fuzz_sidecar() {
    let dir = tmpdir("sidecar");
    let path = Sidecar::path(&dir);
    run("sidecar", 0x221_0005, 1500, |r, last| {
        // a file with two plausible slots, crafted or written by the real code
        let mut buf = if r.chance(2) {
            let _ = fs::remove_file(&path);
            let (mut s, _) = Sidecar::open(&dir).map_err(|e| e.to_string())?;
            for _ in 0..(1 + r.below(4)) {
                let _ = s.save(&refusals(r));
            }
            drop(s);
            fs::read(&path).map_err(|e| e.to_string())?
        } else {
            let mut b = slot_bytes(r.edgy_u64(), &refusals(r));
            b.extend(slot_bytes(r.edgy_u64(), &refusals(r)));
            b
        };
        if r.chance(3) {
            buf = mutate(r, &buf, &[b"\"seq\":", b"\n", b"\0\0\0\0", b"18446744073709551615"]);
        }
        last.extend_from_slice(&buf);
        fs::write(&path, &buf).map_err(|e| e.to_string())?;
        let (mut s, rep) = Sidecar::open(&dir).map_err(|e| format!("open failed: {e}"))?;
        if rep.bad_slots > 2 {
            return Err("more than two bad slots".into());
        }
        if rep.restored.is_none() && rep.seq != 0 {
            return Err("seq without a restored value".into());
        }
        // whatever was there, the next save is what the next open restores
        let next = refusals(r);
        let want_write = rep.restored != Some(next);
        let wrote = s.save(&next).map_err(|e| format!("save failed: {e}"))?;
        if wrote != want_write {
            return Err(format!("save wrote={wrote}, expected {want_write}"));
        }
        drop(s);
        let (_s, rep2) = Sidecar::open(&dir).map_err(|e| e.to_string())?;
        if rep2.restored != Some(next) {
            return Err(format!(
                "saved {next:?} (after seq {}) but reopen restored {:?} at seq {}",
                rep.seq, rep2.restored, rep2.seq
            ));
        }
        Ok(())
    });
    let _ = fs::remove_dir_all(&dir);
}

const REQ_CORPUS: &[&[u8]] = &[
    b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    b"POST /jobs HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"kind\":\"echo\"}",
    b"POST /lease HTTP/1.1\r\nContent-Length: 14\r\n\r\n{\"worker\":\"w\"}",
    b"POST /jobs/1/ack HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"token\":1}",
    b"GET /jobs?state=dead&limit=5 HTTP/1.0\n\n",
];

const HTTP_TOKENS: &[&[u8]] = &[
    b"\r\n",
    b"\n",
    b"Content-Length: ",
    b"content-length:0\r\n",
    b"Content-Length: 99999999\r\n",
    b"Content-Length: +3\r\n",
    b"Content-Length: 1\r\nContent-Length: 2\r\n",
    b"Transfer-Encoding: chunked\r\n",
    b"HTTP/1.1",
    b"HTTP/2",
    b"?a=b&c",
    b" ",
    b":",
    b"\xff\xfe",
];

/// Content-Length values a request declared (header lines before the
/// blank line), as written.
fn declared_lengths(raw: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(raw);
    let mut out = Vec::new();
    for (i, l) in text.split('\n').enumerate() {
        if i == 0 {
            continue;
        }
        let l = l.trim_end_matches('\r');
        if l.trim_end().is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                out.push(v.trim().to_string());
            }
        }
    }
    out
}

#[test]
fn fuzz_http_request() {
    run("http_request", 0x221_0006, 6000, |r, last| {
        let seed_req = *r.pick(REQ_CORPUS);
        let raw = mutate(r, seed_req, HTTP_TOKENS);
        last.extend_from_slice(&raw);
        let _ = http::parse_response(&raw);
        let _ = http::split_url(&String::from_utf8_lossy(&raw));
        if let Ok(req) = http::read_request(&raw[..]) {
            let _ = req.param("a");
            let _ = req.param("");
            if req.method.is_empty() {
                return Err("empty method accepted".into());
            }
            let lens = declared_lengths(&raw);
            // RFC 9112 6.3: Content-Length is 1*DIGIT, and differing
            // duplicates are an unrecoverable framing error.
            if lens.iter().any(|v| v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit())) {
                return Err(format!("non-digit Content-Length accepted: {lens:?}"));
            }
            let mut uniq: Vec<u64> = lens.iter().filter_map(|v| v.parse().ok()).collect();
            uniq.dedup();
            uniq.sort();
            uniq.dedup();
            if uniq.len() > 1 {
                return Err(format!("conflicting Content-Length accepted: {lens:?}"));
            }
            let want = uniq.first().copied().unwrap_or(0);
            if req.body.len() as u64 != want {
                return Err(format!("body {} bytes, Content-Length {want}", req.body.len()));
            }
        }
        Ok(())
    });
}
