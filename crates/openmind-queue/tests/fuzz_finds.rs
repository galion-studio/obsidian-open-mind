// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Regression tests for the slot 221 fuzz finds (`tests/fuzz_queue.rs`).
//! Each test is the minimised input of one find; on the base b71a909 each
//! of them panicked (debug) or returned the wrong answer.

use openmind_queue::engine::{
    Budget, Config, Engine, Job, JobSpec, JobState, Record, Refusal, Stats, GLOBAL, MAX_COST,
};
use openmind_queue::http;
use openmind_queue::refusals::{Refusals, Sidecar, SLOT};
use openmind_queue::store::crc32;
use openmind_queue::vram::{Device, DeviceKind, DeviceState, VramPool};
use std::fs;

fn spec(prio: i32, cost: u64) -> JobSpec {
    JobSpec {
        queue: "q".into(),
        agent: "a".into(),
        priority: prio,
        cost,
        ..JobSpec::default()
    }
}

fn enq(id: u64, s: JobSpec) -> Record {
    Record::Enqueue {
        id,
        now: 1,
        spec: s,
    }
}

/// Find 1 (replay_hostile): two `enqueue` records for one id left a stale
/// ready-index entry; after the ack `lease` indexed `jobs[&id]` and
/// panicked (in release, with panic = "abort", the whole server dies),
/// and `depth()` underflowed.
#[test]
fn duplicate_enqueue_id_is_ignored_and_lease_does_not_panic() {
    let mut e = Engine::new(Config::default());
    e.apply(&enq(1, spec(0, 0)));
    e.apply(&enq(1, spec(5, 0)));
    assert_eq!(e.depth(), (1, 0, 0, 0));
    assert_eq!(e.job(1).unwrap().spec.priority, 0, "first record wins");
    let l = e.lease("q", "w", 10, 100).unwrap();
    e.apply(&l);
    let Record::Lease { id, token, .. } = l else {
        unreachable!()
    };
    let a = e.ack(id, token, None).unwrap();
    e.apply(&a);
    assert!(e.lease("q", "w", 20, 100).is_err());
    assert_eq!(e.depth(), (0, 0, 0, 0));
    assert_eq!(e.agents()["a"].pending, 0);
}

/// Find 1b: a `snapshot` for an id that is already live is ignored too.
#[test]
fn duplicate_snapshot_id_is_ignored() {
    let mut e = Engine::new(Config::default());
    e.apply(&enq(3, spec(1, 0)));
    let mut j = Job {
        id: 3,
        spec: spec(9, 0),
        created_ms: 1,
        state: JobState::Delayed,
        attempts: 1,
        ready_at: 99,
        token: 0,
        worker: String::new(),
        deadline: 0,
        reserved: 0,
        last_error: String::new(),
        device: String::new(),
    };
    e.apply(&Record::Snapshot(Box::new(j.clone())));
    assert_eq!(e.depth(), (1, 0, 0, 0));
    j.id = 4;
    e.apply(&Record::Snapshot(Box::new(j)));
    assert_eq!(e.depth(), (1, 1, 0, 0));
}

/// Find 2 (replay_hostile): `id + 1` / `token + 1` overflowed on a
/// record with u64::MAX; the next enqueue must not reuse a live id.
#[test]
fn max_ids_saturate_and_enqueue_refuses_an_id_in_use() {
    let mut e = Engine::new(Config::default());
    e.apply(&enq(u64::MAX, spec(0, 0)));
    e.apply(&Record::Next {
        id: u64::MAX,
        token: u64::MAX,
    });
    let l = e.lease("q", "w", 5, 10).unwrap();
    e.apply(&l); // token u64::MAX: next_token saturates
    match e.enqueue(spec(0, 0), 6) {
        Err(Refusal::Full(m)) => assert!(m.contains("in use"), "{m}"),
        other => panic!("expected backpressure, got {other:?}"),
    }
}

/// Find 3 (job_body, reachable over HTTP): costs near u64::MAX overflowed
/// the reservation sums. A cost over MAX_COST is now refused at the door.
#[test]
fn cost_over_max_cost_is_refused_and_max_cost_sums_exactly() {
    let mut e = Engine::new(Config::default());
    match e.enqueue(spec(0, u64::MAX), 1) {
        Err(Refusal::BadRequest(m)) => assert!(m.contains("per-job limit"), "{m}"),
        other => panic!("expected 400, got {other:?}"),
    }
    for _ in 0..3 {
        let r = e.enqueue(spec(0, MAX_COST), 1).unwrap();
        e.apply(&r);
    }
    for _ in 0..3 {
        let l = e.lease("q", "w", 2, 100).unwrap();
        e.apply(&l);
    }
    assert_eq!(e.global().reserved, 3 * MAX_COST);
    let mut e2 = Engine::new(Config::default());
    for r in e.snapshot() {
        e2.apply(&r);
    }
    assert_eq!(e2.global(), e.global());
}

/// Find 4 (job_body): with cap u64::MAX the saturated sum `1 + u64::MAX`
/// compared equal to the cap and passed; an overflowing sum never fits.
#[test]
fn overflowing_budget_sum_never_fits() {
    let b = Budget {
        cap: Some(u64::MAX),
        spent: 1,
        reserved: 0,
    };
    assert!(!b.fits(u64::MAX));
    assert!(b.fits(u64::MAX - 1));
    let b = Budget {
        cap: Some(10),
        spent: 4,
        reserved: 6,
    };
    assert!(b.fits(0));
    assert!(!b.fits(1));
}

/// Find 5 (job_body, reachable over HTTP via POST /vram/devices + GET
/// /stats): summing `cents_per_hour` of two devices overflowed.
#[test]
fn vram_summary_saturates() {
    let mut p = VramPool::default();
    for id in ["g1", "g2"] {
        p.upsert(Device {
            id: id.into(),
            kind: DeviceKind::Gpu,
            vram_gb: 24,
            provider: String::new(),
            cents_per_hour: u64::MAX,
            state: DeviceState::Approved,
            healthy: true,
        });
    }
    p.hold("g1", u32::MAX);
    p.hold("g1", u32::MAX);
    let s = p.summary();
    assert_eq!(s.approved_cents_per_hour, u64::MAX);
    assert_eq!(s.approved_gb, 48);
}

fn slot(seq: u64, c: &Refusals) -> Vec<u8> {
    let json = format!(
        "{{\"seq\":{seq},\"c\":{}}}",
        serde_json::to_string(c).unwrap()
    );
    let mut v = format!("{:08x} {json}\n", crc32(json.as_bytes())).into_bytes();
    v.resize(SLOT, 0);
    v
}

/// Find 6 (sidecar): a CRC-valid slot with seq u64::MAX overflowed the
/// next save (debug) or wrapped it to seq 0 (release), so the stale slot
/// won every later open and refusal counts stopped persisting.
#[test]
fn sidecar_at_max_seq_is_rebased_and_keeps_persisting() {
    let d = std::env::temp_dir().join(format!("q221-finds-sidecar-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let old = Refusals {
        quota_refusals: 7,
        ..Refusals::default()
    };
    let mut b = slot(u64::MAX - 1, &Refusals::default());
    b.extend(slot(u64::MAX, &old));
    fs::write(Sidecar::path(&d), &b).unwrap();
    let (mut s, rep) = Sidecar::open(&d).unwrap();
    assert_eq!((rep.restored, rep.seq), (Some(old), u64::MAX));
    assert_eq!(s.seq(), 2, "rebased on open");
    let new = Refusals {
        quota_refusals: 8,
        ..old
    };
    assert!(s.save(&new).unwrap());
    drop(s);
    let (_s, rep) = Sidecar::open(&d).unwrap();
    assert_eq!((rep.restored, rep.seq, rep.bad_slots), (Some(new), 3, 0));
    let _ = fs::remove_dir_all(&d);
}

/// Find 6b: restored counters at u64::MAX no longer overflow on the next
/// refusal.
#[test]
fn refusal_counters_at_max_saturate() {
    let mut e = Engine::new(Config::default());
    e.stats = Stats {
        stale_acks: u64::MAX,
        ..Stats::default()
    };
    assert_eq!(e.ack(42, 1, None), Err(Refusal::NotFound));
    assert_eq!(e.stats.stale_acks, u64::MAX);
}

/// Find 7 (http_request): differing duplicate Content-Length was "last
/// one wins", and `+N` parsed as N. RFC 9112 6.3: reject both.
#[test]
fn content_length_must_be_digits_and_agree() {
    let bad: &[&[u8]] = &[
        b"POST /jobs HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n{}",
        b"POST /jobs HTTP/1.1\r\nContent-Length: +2\r\n\r\n{}",
        b"POST /jobs HTTP/1.1\r\nContent-Length: 2 2\r\n\r\n{}",
        b"POST /jobs HTTP/1.1\r\nContent-Length:\r\n\r\n{}",
    ];
    for raw in bad {
        assert!(
            http::read_request(&raw[..]).is_err(),
            "accepted {}",
            String::from_utf8_lossy(raw)
        );
    }
    let ok = b"POST /jobs HTTP/1.1\r\ncontent-length: 2\r\nContent-Length:  2 \r\n\r\n{}";
    assert_eq!(http::read_request(&ok[..]).unwrap().body, b"{}");
}

/// The global scope is never an agent: enqueue refuses agent "*" at the
/// door (D-221-3 documents why a log job owned by "*" is not fixed-point).
#[test]
fn agent_star_is_refused_at_the_door() {
    let mut e = Engine::new(Config::default());
    let s = JobSpec {
        agent: GLOBAL.into(),
        ..JobSpec::default()
    };
    assert!(matches!(e.enqueue(s, 1), Err(Refusal::BadRequest(_))));
}
