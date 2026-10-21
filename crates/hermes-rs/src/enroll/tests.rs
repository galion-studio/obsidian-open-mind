// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests for enrollment: a test device signs exactly like omnode.

use super::verify::{device_id, fingerprint, hex, PAYLOAD_TTL};
use super::*;
use ed25519_dalek::{Signer, SigningKey};

const URL: &str = "http://127.0.0.1:19320/openmind/enroll";
const NOW: i64 = 1_792_346_400; // 2026-10-18T20:00:00+02:00

struct Dev {
    key: SigningKey,
}

impl Dev {
    fn new(seed: u8) -> Dev {
        Dev {
            key: SigningKey::from_bytes(&[seed; 32]),
        }
    }
    fn id(&self) -> String {
        device_id(&self.key.verifying_key())
    }
    fn fp(&self) -> String {
        fingerprint(&self.key.verifying_key())
    }
    fn payload(&self, nonce: &str, created: i64, caps: &[&str]) -> Value {
        let vk = self.key.verifying_key();
        let rfc = crate::time::zagreb_rfc3339;
        json!({
            "v": 1, "kind": "openmind-pair", "device_id": device_id(&vk),
            "public_key": hex(vk.as_bytes()), "fingerprint": fingerprint(&vk),
            "network_hash": null, "network_trusted": false, "capabilities": caps,
            "arch": "x86_64", "cores": 4, "ram_mb": 2048, "platform": "linux",
            "enroll_url": URL, "nonce": nonce, "created": rfc(created),
            "expires": rfc(created + PAYLOAD_TTL), "expires_unix": created + PAYLOAD_TTL,
        })
    }
    fn sign_payload(&self, p: &Value) -> String {
        let caps: Vec<&str> = p["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        let s = [
            "omnode-pair-v1",
            p["device_id"].as_str().unwrap(),
            p["public_key"].as_str().unwrap(),
            p["network_hash"].as_str().unwrap_or("-"),
            &caps.join(","),
            p["arch"].as_str().unwrap(),
            &p["cores"].to_string(),
            &p["ram_mb"].to_string(),
            p["enroll_url"].as_str().unwrap(),
            p["nonce"].as_str().unwrap(),
            p["created"].as_str().unwrap(),
            p["expires"].as_str().unwrap(),
        ]
        .join("|");
        hex(&self.key.sign(s.as_bytes()).to_bytes())
    }
    fn doc(&self, nonce: &str, created: i64, caps: &[&str]) -> Value {
        let p = self.payload(nonce, created, caps);
        let sig = self.sign_payload(&p);
        json!({"payload": p, "signature": sig, "signed_string": "ignored"})
    }
    fn claim(&self, nonce: &str, ts: i64) -> Value {
        let sig = hex(&self
            .key
            .sign(claim_canonical(&self.id(), nonce, ts, URL).as_bytes())
            .to_bytes());
        json!({"device_id": self.id(), "nonce": nonce, "ts": ts, "signature": sig})
    }
}

fn n(i: u8) -> String {
    format!("{:024x}", i as u128 + 0xabc0_0000)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "hermes-enroll-test-{tag}-{}-{}",
        std::process::id(),
        crate::new_id("")
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg(dir: &Path) -> EnrollCfg {
    EnrollCfg {
        url: URL.into(),
        store: dir.join("rs-enroll.json"),
        max_pending: 3,
        nonce_ttl: 900,
        pending_ttl: 3600,
        cred_ttl: 86_400,
        rate_per_min: 1000,
        tunnel_ping: 1,
        max_tunnels: 2,
        tunnel_life: 3600,
    }
}

fn post(path: &str, body: &Value) -> Request {
    Request {
        method: "POST".into(),
        path: path.into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: body.to_string().into_bytes(),
        ..Request::default()
    }
}

fn get(path: &str, query: &str, bearer: Option<&str>) -> Request {
    Request {
        method: "GET".into(),
        path: path.into(),
        query: query.into(),
        headers: bearer
            .map(|b| vec![("authorization".into(), format!("Bearer {b}"))])
            .unwrap_or_default(),
        ..Request::default()
    }
}

fn status_body(r: Response) -> (u16, Value) {
    match r {
        Response::Full { status, body, .. } => (status, serde_json::from_slice(&body).unwrap()),
        _ => panic!("stream"),
    }
}

/// Enroll + approve + claim; returns the token.
fn enrolled(e: &Enroll, d: &Dev, base: u8) -> String {
    let (s, _) = status_body(e.handle(
        &post(ENROLL_PATH, &d.doc(&n(base), NOW, &["cpu", "sandbox"])),
        false,
        NOW,
    ));
    assert_eq!(s, 202);
    let (s, _) = status_body(e.handle(
        &post(
            "/openmind/enroll/approve",
            &json!({"device_id": d.id(), "fingerprint": d.fp()}),
        ),
        true,
        NOW,
    ));
    assert_eq!(s, 200);
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &d.claim(&n(base + 1), NOW)),
        false,
        NOW,
    ));
    assert_eq!(s, 200, "{b}");
    b["token"].as_str().unwrap().to_string()
}

#[test]
fn full_flow_and_scoped_credential() {
    let dir = tmp("flow");
    let e = Enroll::open(cfg(&dir)).unwrap();
    let a = Dev::new(1);
    // claim before anything: not enrolled
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/claim", &a.claim(&n(50), NOW)),
        false,
        NOW,
    ));
    assert_eq!(s, 403);
    let (s, b) = status_body(e.handle(
        &post(
            ENROLL_PATH,
            &a.doc(&n(1), NOW, &["cpu", "money", "sandbox"]),
        ),
        false,
        NOW,
    ));
    assert_eq!(s, 202, "{b}");
    assert_eq!(b["fingerprint"], a.fp());
    // pending: claim answers 202 pending, no token
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &a.claim(&n(2), NOW)),
        false,
        NOW,
    ));
    assert_eq!(s, 202);
    assert!(b.get("token").is_none());
    // owner routes need the owner flag
    let appr = json!({"device_id": a.id(), "fingerprint": a.fp()});
    let (s, _) = status_body(e.handle(&post("/openmind/enroll/approve", &appr), false, NOW));
    assert_eq!(s, 401);
    // wrong fingerprint refused
    let bad = json!({"device_id": a.id(), "fingerprint": "SHA256:0000"});
    let (s, _) = status_body(e.handle(&post("/openmind/enroll/approve", &bad), true, NOW));
    assert_eq!(s, 409);
    let (s, b) = status_body(e.handle(&post("/openmind/enroll/approve", &appr), true, NOW));
    assert_eq!(s, 200);
    assert_eq!(b["scopes"], json!(["cpu", "sandbox"]));
    assert_eq!(b["denied"], json!(["money"]));
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &a.claim(&n(3), NOW)),
        false,
        NOW,
    ));
    assert_eq!(s, 200);
    let tok = b["token"].as_str().unwrap().to_string();
    assert!(tok.starts_with(TOKEN_PREFIX));
    // token never stored in clear
    let disk = std::fs::read_to_string(dir.join("rs-enroll.json")).unwrap();
    assert!(!disk.contains(&tok));
    assert!(disk.contains(&sha256_hex(&tok)));
    let (s, b) = status_body(e.handle(&get("/openmind/node/whoami", "", Some(&tok)), false, NOW));
    assert_eq!(s, 200);
    assert_eq!(b["device_id"], a.id());
    let (s, _) = status_body(e.handle(
        &get("/openmind/node/allowed", "scope=cpu", Some(&tok)),
        false,
        NOW,
    ));
    assert_eq!(s, 200);
    let (s, _) = status_body(e.handle(
        &get("/openmind/node/allowed", "scope=money", Some(&tok)),
        false,
        NOW,
    ));
    assert_eq!(s, 403);
    // owner device list never shows the hash
    let (s, b) = status_body(e.handle(&get("/openmind/enroll/devices", "", None), true, NOW));
    assert_eq!(s, 200);
    assert!(!b.to_string().contains(&sha256_hex(&tok)));
    // credential expiry
    assert_eq!(e.authorize(&tok, None, NOW + 86_401), Err(401));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn refuses_replay_expired_tampered_unenrolled() {
    let dir = tmp("refuse");
    let e = Enroll::open(cfg(&dir)).unwrap();
    let a = Dev::new(2);
    let doc = a.doc(&n(1), NOW, &["cpu"]);
    assert_eq!(e.handle(&post(ENROLL_PATH, &doc), false, NOW).status(), 202);
    // replay of the very same document
    let (s, b) = status_body(e.handle(&post(ENROLL_PATH, &doc), false, NOW + 5));
    assert_eq!(
        (s, b["error"].as_str().unwrap()),
        (409, "nonce already used")
    );
    // expired
    let old = a.doc(&n(2), NOW - PAYLOAD_TTL - 200, &["cpu"]);
    let (s, b) = status_body(e.handle(&post(ENROLL_PATH, &old), false, NOW));
    assert_eq!(s, 400);
    assert!(b["error"].as_str().unwrap().contains("expired"));
    // tampered capabilities with the original signature
    let mut t = a.doc(&n(3), NOW, &["cpu"]);
    t["payload"]["capabilities"] = json!(["cpu", "admin"]);
    let (s, b) = status_body(e.handle(&post(ENROLL_PATH, &t), false, NOW));
    assert_eq!(s, 400);
    assert!(b["error"].as_str().unwrap().contains("signature"));
    // wrong URL
    let mut w = a.payload(&n(4), NOW, &["cpu"]);
    w["enroll_url"] = json!("https://other.example.invalid/openmind/enroll");
    let sig = a.sign_payload(&w);
    let (s, b) = status_body(e.handle(
        &post(ENROLL_PATH, &json!({"payload": w, "signature": sig})),
        false,
        NOW,
    ));
    assert_eq!(s, 400);
    assert!(b["error"].as_str().unwrap().contains("enroll_url"));
    // unenrolled device: forged token and claim
    let b2 = Dev::new(3);
    let fake = format!("{TOKEN_PREFIX}{}", "ab".repeat(32));
    assert_eq!(
        e.handle(&get("/openmind/node/whoami", "", Some(&fake)), false, NOW)
            .status(),
        401
    );
    assert_eq!(
        e.handle(&get("/openmind/node/whoami", "", None), false, NOW)
            .status(),
        401
    );
    assert_eq!(
        e.handle(
            &post("/openmind/enroll/claim", &b2.claim(&n(9), NOW)),
            false,
            NOW
        )
        .status(),
        403
    );
    // claim signed by another key for a known device
    let mut c = b2.claim(&n(10), NOW);
    c["device_id"] = json!(a.id());
    assert_eq!(
        e.handle(&post("/openmind/enroll/claim", &c), false, NOW)
            .status(),
        401
    );
    // stale claim timestamp, replayed claim nonce
    assert_eq!(
        e.handle(
            &post("/openmind/enroll/claim", &a.claim(&n(11), NOW - 500)),
            false,
            NOW
        )
        .status(),
        400
    );
    let c = a.claim(&n(12), NOW);
    assert_eq!(
        e.handle(&post("/openmind/enroll/claim", &c), false, NOW)
            .status(),
        202
    );
    assert_eq!(
        e.handle(&post("/openmind/enroll/claim", &c), false, NOW)
            .status(),
        409
    );
    // a pairing nonce can not be reused as a claim nonce
    assert_eq!(
        e.handle(
            &post("/openmind/enroll/claim", &a.claim(&n(1), NOW)),
            false,
            NOW
        )
        .status(),
        409
    );
    // wrong content type, oversized body
    let mut r = post(ENROLL_PATH, &doc);
    r.headers.clear();
    assert_eq!(e.handle(&r, false, NOW).status(), 415);
    let mut r = post(ENROLL_PATH, &doc);
    r.body = vec![b' '; MAX_BODY + 1];
    assert_eq!(e.handle(&r, false, NOW).status(), 413);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn revoke_and_persistence() {
    let dir = tmp("revoke");
    let a = Dev::new(4);
    let b = Dev::new(5);
    let (ta, tb) = {
        let e = Enroll::open(cfg(&dir)).unwrap();
        (enrolled(&e, &a, 1), enrolled(&e, &b, 10))
    };
    // "restart": a new instance reads the same store
    let e = Enroll::open(cfg(&dir)).unwrap();
    assert!(e.authorize(&ta, None, NOW).is_ok());
    assert!(e.authorize(&tb, Some("sandbox"), NOW).is_ok());
    // replayed nonce from before the restart is still refused
    let again = a.doc(&n(1), NOW, &["cpu", "sandbox"]);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &again), false, NOW).status(),
        409
    );
    // already enrolled device with a fresh nonce
    let fresh = a.doc(&n(30), NOW, &["cpu"]);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &fresh), false, NOW).status(),
        409
    );
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/revoke", &json!({"device_id": a.id()})),
        true,
        NOW,
    ));
    assert_eq!(s, 200);
    assert_eq!(e.authorize(&ta, None, NOW), Err(401));
    assert!(
        e.authorize(&tb, None, NOW).is_ok(),
        "other device unaffected"
    );
    // revoked device: no new credential, no re-enrollment
    assert_eq!(
        e.handle(
            &post("/openmind/enroll/claim", &a.claim(&n(31), NOW)),
            false,
            NOW
        )
        .status(),
        403
    );
    assert_eq!(
        e.handle(
            &post(ENROLL_PATH, &a.doc(&n(32), NOW, &["cpu"])),
            false,
            NOW
        )
        .status(),
        403
    );
    drop(e);
    let e = Enroll::open(cfg(&dir)).unwrap();
    assert_eq!(
        e.authorize(&ta, None, NOW),
        Err(401),
        "revocation survives restart"
    );
    assert!(e.authorize(&tb, None, NOW).is_ok());
    // a new claim replaces the old credential
    let (s, bb) = status_body(e.handle(
        &post("/openmind/enroll/claim", &b.claim(&n(40), NOW)),
        false,
        NOW,
    ));
    assert_eq!(s, 200);
    assert_eq!(e.authorize(&tb, None, NOW), Err(401));
    assert!(e
        .authorize(bb["token"].as_str().unwrap(), None, NOW)
        .is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn store_fails_closed() {
    let dir = tmp("store");
    let c = cfg(&dir);
    std::fs::write(&c.store, b"{not json").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&c.store, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let err = Enroll::open(c.clone()).err().unwrap();
    assert!(err.contains("corrupt"), "{err}");
    // the corrupt file is left alone, never reset
    assert_eq!(std::fs::read(&c.store).unwrap(), b"{not json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            &c.store,
            br#"{"v":1,"nonces":{},"pending":[],"devices":{},"flags":[]}"#,
        )
        .unwrap();
        std::fs::set_permissions(&c.store, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Enroll::open(c.clone()).err().unwrap().contains("600"));
    }
    // unknown fields refused
    std::fs::remove_file(&c.store).unwrap();
    write_atomic(
        &c.store,
        br#"{"v":1,"nonces":{},"pending":[],"devices":{},"flags":[],"x":1}"#,
    )
    .unwrap();
    assert!(Enroll::open(c.clone()).is_err());
    // unwritable directory: refuse to start
    let bad = EnrollCfg {
        store: dir.join("missing-dir").join("s.json"),
        ..c
    };
    assert!(Enroll::open(bad).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn write_failure_changes_nothing() {
    let dir = tmp("wfail");
    let c = cfg(&dir);
    let e = Enroll::open(c.clone()).unwrap();
    let a = Dev::new(6);
    // make the store's directory unusable: replace the store by a directory
    // so rename fails.
    std::fs::remove_file(&c.store).unwrap();
    std::fs::create_dir(&c.store).unwrap();
    std::fs::write(c.store.join("x"), b"x").unwrap();
    let (s, _) =
        status_body(e.handle(&post(ENROLL_PATH, &a.doc(&n(1), NOW, &["cpu"])), false, NOW));
    assert_eq!(s, 503);
    assert!(e.snapshot().nonces.is_empty());
    assert!(e.snapshot().pending.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pending_cap_and_rate_limit() {
    let dir = tmp("caps");
    let mut c = cfg(&dir);
    c.rate_per_min = 5;
    let e = Enroll::open(c).unwrap();
    for i in 0..3u8 {
        let d = Dev::new(20 + i);
        assert_eq!(
            e.handle(&post(ENROLL_PATH, &d.doc(&n(i), NOW, &["cpu"])), false, NOW)
                .status(),
            202
        );
    }
    let d = Dev::new(30);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &d.doc(&n(9), NOW, &["cpu"])), false, NOW)
            .status(),
        429
    );
    // 5 per minute total, then 429 until the window moves
    assert_eq!(
        e.handle(
            &post(ENROLL_PATH, &d.doc(&n(10), NOW, &["cpu"])),
            false,
            NOW
        )
        .status(),
        429
    );
    let (s, b) = status_body(e.handle(
        &post(ENROLL_PATH, &d.doc(&n(11), NOW, &["cpu"])),
        false,
        NOW,
    ));
    assert_eq!(
        (s, b["error"].as_str().unwrap()),
        (429, "rate limited, try again in a minute")
    );
    // reject frees a slot
    let r = json!({"device_id": Dev::new(20).id()});
    assert_eq!(
        e.handle(&post("/openmind/enroll/reject", &r), true, NOW + 61)
            .status(),
        200
    );
    assert_eq!(
        e.handle(
            &post(ENROLL_PATH, &d.doc(&n(12), NOW + 61, &["cpu"])),
            false,
            NOW + 61
        )
        .status(),
        202
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn config_is_fail_closed() {
    let home = tmp("cfg");
    let base = crate::config::Config::defaults(&home);
    let env_of = |pairs: Vec<(&'static str, &'static str)>| {
        move |k: &str| {
            pairs
                .iter()
                .find(|(a, _)| *a == k)
                .map(|(_, v)| v.to_string())
        }
    };
    assert_eq!(EnrollCfg::from_env(&base, &env_of(vec![])), Ok(None));
    assert_eq!(
        EnrollCfg::from_env(&base, &env_of(vec![("HERMES_RS_ENROLL", "0")])),
        Ok(None)
    );
    assert!(EnrollCfg::from_env(&base, &env_of(vec![("HERMES_RS_ENROLL", "maybe")])).is_err());
    assert!(EnrollCfg::from_env(&base, &env_of(vec![("HERMES_RS_ENROLL", "1")])).is_err());
    let ok = EnrollCfg::from_env(
        &base,
        &env_of(vec![
            ("HERMES_RS_ENROLL", "1"),
            ("HERMES_RS_ENROLL_URL", URL),
        ]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(ok.store, home.join("rs-enroll.json"));
    for bad in [
        "http://hermes.example.invalid/openmind/enroll",
        "https://hermes.example.invalid/other",
        "ftp://x/openmind/enroll",
        "https://hermes.example.invalid/openmind/enroll/x",
    ] {
        let leaked: &'static str = Box::leak(bad.to_string().into_boxed_str());
        assert!(
            EnrollCfg::from_env(
                &base,
                &env_of(vec![
                    ("HERMES_RS_ENROLL", "1"),
                    ("HERMES_RS_ENROLL_URL", leaked)
                ])
            )
            .is_err(),
            "{bad}"
        );
    }
    assert!(EnrollCfg::from_env(
        &base,
        &env_of(vec![
            ("HERMES_RS_ENROLL", "1"),
            ("HERMES_RS_ENROLL_URL", URL),
            ("HERMES_RS_ENROLL_NONCE_TTL", "60")
        ])
    )
    .is_err());
    let mut public = base.clone();
    public.host = "0.0.0.0".into();
    let https = "https://hermes.example.invalid/openmind/enroll";
    assert!(EnrollCfg::from_env(
        &public,
        &env_of(vec![
            ("HERMES_RS_ENROLL", "1"),
            ("HERMES_RS_ENROLL_URL", https)
        ])
    )
    .is_err());
    assert!(EnrollCfg::from_env(
        &public,
        &env_of(vec![
            ("HERMES_RS_ENROLL", "1"),
            ("HERMES_RS_ENROLL_URL", https),
            ("HERMES_RS_ENROLL_ALLOW_PUBLIC", "1")
        ])
    )
    .unwrap()
    .is_some());
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn zagreb_strings_match_omnode() {
    // Same instants as omnode's zagreb.rs tests: check 6 compares these strings.
    assert_eq!(
        crate::time::zagreb_rfc3339(1_792_346_400),
        "2026-10-18T20:00:00+02:00"
    );
    assert_eq!(
        crate::time::zagreb_rfc3339(1_798_113_600),
        "2026-12-24T13:00:00+01:00"
    );
}

// ------------------------------------------------ omnode-enroll additions

impl Dev {
    fn status(&self, ts: i64) -> Value {
        let sig = hex(&self
            .key
            .sign(status_canonical(&self.id(), ts, URL).as_bytes())
            .to_bytes());
        json!({"device_id": self.id(), "ts": ts, "signature": sig})
    }
    fn code(&self, nonce: &str) -> String {
        pairing_code(&hex(self.key.verifying_key().as_bytes()), nonce)
    }
    fn answer(&self, session: &str, id: &str, challenge: &str) -> Value {
        let sig = hex(&self
            .key
            .sign(tunnel_canonical(&self.id(), session, id, challenge, URL).as_bytes())
            .to_bytes());
        json!({"session": session, "id": id, "signature": sig})
    }
}

fn poll(e: &Enroll, d: &Dev, now: i64) -> (u16, Value) {
    status_body(e.handle(&post("/openmind/enroll/status", &d.status(now)), false, now))
}

#[test]
fn pairing_code_shape_and_normalising() {
    let d = Dev::new(40);
    let c = d.code(&n(1));
    assert_eq!(c.len(), 9);
    assert_eq!(&c[4..5], "-");
    assert!(c
        .chars()
        .all(|ch| ch == '-' || CODE_ALPHABET.contains(&(ch as u8))));
    // stable for the same (key, nonce), different for another nonce
    assert_eq!(c, d.code(&n(1)));
    assert_ne!(c, d.code(&n(2)));
    assert_eq!(normalize_code(&c.to_lowercase()), Some(c.clone()));
    assert_eq!(normalize_code(&c.replace('-', " ")), Some(c.clone()));
    assert_eq!(normalize_code("o0il-ABCD").as_deref(), Some("0011-ABCD"));
    for bad in [
        "",
        "ABC",
        "ABCD-EFGH-J",
        "ABCD-EFGU",
        "ABCD_EFGH",
        "ÄBCD-EFGH",
    ] {
        assert_eq!(normalize_code(bad), None, "{bad}");
    }
    // pinned vector: omnode computes the same string (edge/openmind-node tests)
    assert_eq!(
        pairing_code(&"11".repeat(32), &"22".repeat(12)),
        "J820-7XVQ"
    );
}

#[test]
fn approve_by_code_then_claim_and_status() {
    let dir = tmp("code");
    let e = Enroll::open(cfg(&dir)).unwrap();
    let d = Dev::new(41);
    let (s, b) =
        status_body(e.handle(&post(ENROLL_PATH, &d.doc(&n(1), NOW, &["cpu"])), false, NOW));
    assert_eq!(s, 202);
    let code = d.code(&n(1));
    assert_eq!(b["code"], json!(code));
    assert_eq!(b["pending_expires_unix"], json!(NOW + 3600));
    // pending: status poll says so and claim gives no token
    let (s, b) = poll(&e, &d, NOW + 1);
    assert_eq!((s, b["status"].as_str().unwrap()), (202, "pending"));
    assert_eq!(b["code"], json!(code));
    // the owner list shows the code and the fingerprint
    let (s, b) = status_body(e.handle(&get("/openmind/enroll/pending", "", None), true, NOW));
    assert_eq!(s, 200);
    assert_eq!(b["pending"][0]["code"], json!(code));
    assert_eq!(b["pending"][0]["fingerprint"], json!(d.fp()));
    // owner routes stay closed without the key, also for code approval
    let ap = json!({"code": code.to_lowercase()});
    assert_eq!(
        e.handle(&post("/openmind/enroll/approve", &ap), false, NOW)
            .status(),
        401
    );
    // wrong code, malformed code, code + wrong fingerprint
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/approve", &json!({"code": "0000-0000"})),
        true,
        NOW,
    ));
    assert_eq!(s, 404);
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/approve", &json!({"code": "nope"})),
        true,
        NOW,
    ));
    assert_eq!(s, 400);
    let (s, _) = status_body(e.handle(
        &post(
            "/openmind/enroll/approve",
            &json!({"code": code, "fingerprint": "SHA256:0000"}),
        ),
        true,
        NOW,
    ));
    assert_eq!(s, 409);
    // right code (lower case, as typed)
    let (s, b) = status_body(e.handle(&post("/openmind/enroll/approve", &ap), true, NOW + 2));
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "approved"), "{b}");
    let (s, b) = poll(&e, &d, NOW + 3);
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "approved"));
    assert_eq!(b["credential"], json!(false));
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &d.claim(&n(2), NOW + 4)),
        false,
        NOW + 4,
    ));
    assert_eq!(s, 200, "{b}");
    assert!(b["token"].as_str().unwrap().starts_with(TOKEN_PREFIX));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn status_poll_is_signed_and_writes_nothing() {
    let dir = tmp("status");
    let e = Enroll::open(cfg(&dir)).unwrap();
    let d = Dev::new(42);
    let other = Dev::new(43);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &d.doc(&n(1), NOW, &["cpu"])), false, NOW)
            .status(),
        202
    );
    let before = std::fs::read(&e.cfg.store).unwrap();
    // unknown device
    assert_eq!(poll(&e, &other, NOW).0, 403);
    // signed by another key
    let mut forged = other.status(NOW);
    forged["device_id"] = json!(d.id());
    assert_eq!(
        status_body(e.handle(&post("/openmind/enroll/status", &forged), false, NOW)).0,
        401
    );
    // stale timestamp, unknown field
    assert_eq!(poll(&e, &d, NOW).0, 202);
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/status", &d.status(NOW - 500)),
        false,
        NOW,
    ));
    assert_eq!(s, 400);
    let mut x = d.status(NOW);
    x["token"] = json!("x");
    assert_eq!(
        status_body(e.handle(&post("/openmind/enroll/status", &x), false, NOW)).0,
        400
    );
    // never a store write, never a token
    assert_eq!(std::fs::read(&e.cfg.store).unwrap(), before);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn owner_refusal_is_told_to_the_device() {
    let dir = tmp("reject");
    let e = Enroll::open(cfg(&dir)).unwrap();
    let d = Dev::new(44);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &d.doc(&n(1), NOW, &["cpu"])), false, NOW)
            .status(),
        202
    );
    // reject needs the key
    let rj = json!({"code": d.code(&n(1))});
    assert_eq!(
        e.handle(&post("/openmind/enroll/reject", &rj), false, NOW)
            .status(),
        401
    );
    let (s, b) = status_body(e.handle(&post("/openmind/enroll/reject", &rj), true, NOW + 1));
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "rejected"));
    // device: status and claim both say rejected (not "not enrolled")
    let (s, b) = poll(&e, &d, NOW + 2);
    assert_eq!((s, b["status"].as_str().unwrap()), (403, "rejected"));
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &d.claim(&n(2), NOW + 3)),
        false,
        NOW + 3,
    ));
    assert_eq!((s, b["status"].as_str().unwrap()), (403, "rejected"));
    assert!(b.get("token").is_none());
    // a refused request can not be approved afterwards
    let (s, _) = status_body(e.handle(&post("/openmind/enroll/approve", &rj), true, NOW + 4));
    assert_eq!(s, 404);
    // the outcome survives a restart (same persistence as the rest)
    drop(e);
    let e = Enroll::open(cfg(&dir)).unwrap();
    assert_eq!(poll(&e, &d, NOW + 5).1["status"], json!("rejected"));
    let (_, b) = status_body(e.handle(&get("/openmind/enroll/pending", "", None), true, NOW + 5));
    assert_eq!(b["recent"][0]["status"], json!("rejected"));
    // a fresh request starts over (new nonce, new code)
    assert_eq!(
        e.handle(
            &post(ENROLL_PATH, &d.doc(&n(3), NOW + 6, &["cpu"])),
            false,
            NOW + 6
        )
        .status(),
        202
    );
    assert_eq!(poll(&e, &d, NOW + 7).1["status"], json!("pending"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unanswered_request_times_out() {
    let dir = tmp("timeout");
    let c = EnrollCfg {
        pending_ttl: 60,
        ..cfg(&dir)
    };
    let e = Enroll::open(c).unwrap();
    let d = Dev::new(45);
    assert_eq!(
        e.handle(&post(ENROLL_PATH, &d.doc(&n(1), NOW, &["cpu"])), false, NOW)
            .status(),
        202
    );
    assert_eq!(poll(&e, &d, NOW + 59).1["status"], json!("pending"));
    // at the TTL: expired for the device, gone from the owner list
    let (s, b) = poll(&e, &d, NOW + 60);
    assert_eq!((s, b["status"].as_str().unwrap()), (410, "expired"));
    let (_, b) = status_body(e.handle(&get("/openmind/enroll/pending", "", None), true, NOW + 61));
    assert_eq!(b["pending"].as_array().unwrap().len(), 0);
    assert_eq!(b["recent"][0]["status"], json!("expired"));
    // the owner can no longer approve it
    let (s, _) = status_body(e.handle(
        &post(
            "/openmind/enroll/approve",
            &json!({"device_id": d.id(), "fingerprint": d.fp()}),
        ),
        true,
        NOW + 62,
    ));
    assert_eq!(s, 404);
    // claim says expired (410) and writes the outcome to the store
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &d.claim(&n(2), NOW + 63)),
        false,
        NOW + 63,
    ));
    assert_eq!((s, b["status"].as_str().unwrap()), (410, "expired"));
    let disk = std::fs::read_to_string(&e.cfg.store).unwrap();
    assert!(disk.contains("\"expired\""), "{disk}");
    // outcomes are forgotten after OUTCOME_TTL
    assert_eq!(poll(&e, &d, NOW + 63 + OUTCOME_TTL + 1).0, 403);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn old_store_without_outcomes_or_codes_still_opens() {
    let dir = tmp("compat");
    let c = cfg(&dir);
    let d = Dev::new(46);
    let p = json!({"device_id": d.id(), "public_key": hex(d.key.verifying_key().as_bytes()),
        "fingerprint": d.fp(), "capabilities": ["cpu"], "network_hash": null, "arch": "x86_64",
        "cores": 4, "ram_mb": 2048, "claimed_platform": "linux", "claimed_network_trusted": false,
        "requested_at": "2026-10-18T20:00:00+02:00", "requested_unix": NOW});
    let old = json!({"v": 1, "nonces": {}, "pending": [p], "devices": {}, "flags": []});
    write_atomic(&c.store, old.to_string().as_bytes()).unwrap();
    let e = Enroll::open(c.clone()).unwrap();
    assert_eq!(e.snapshot().pending[0].code, "");
    // no code: approval by code finds nothing, by fingerprint still works
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/approve", &json!({"code": d.code(&n(1))})),
        true,
        NOW,
    ));
    assert_eq!(s, 404);
    let (s, _) = status_body(e.handle(
        &post(
            "/openmind/enroll/approve",
            &json!({"device_id": d.id(), "fingerprint": d.fp()}),
        ),
        true,
        NOW,
    ));
    assert_eq!(s, 200);
    // a bad outcome status in the store is refused (fail closed)
    let bad = json!({"v": 1, "nonces": {}, "pending": [], "devices": {}, "flags": [],
        "outcomes": [{"device_id": "x", "public_key": "y", "fingerprint": "z", "status": "approved",
                      "at": "a", "until_unix": 1}]});
    write_atomic(&c.store, bad.to_string().as_bytes()).unwrap();
    assert!(Enroll::open(c).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// A `Write` that hands every chunk to the test thread.
struct Pipe(std::sync::mpsc::Sender<Vec<u8>>);
impl std::io::Write for Pipe {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .send(b.to_vec())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Start the tunnel stream in a thread; returns a frame reader.
struct Frames {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: String,
    th: Option<std::thread::JoinHandle<()>>,
}
impl Frames {
    fn open(e: &Arc<Enroll>, token: &str) -> Result<Frames, (u16, Value)> {
        let r = e.route(
            &get(TUNNEL_PATH, "", Some(token)),
            false,
            crate::time::now_secs(),
        );
        let Response::Stream { status, writer, .. } = r else {
            return Err(status_body(r));
        };
        assert_eq!(status, 200);
        let (tx, rx) = std::sync::mpsc::channel();
        let th = std::thread::spawn(move || {
            let mut p = Pipe(tx);
            let mut w = crate::http::SseWriter::new(&mut p);
            writer(&mut w);
        });
        Ok(Frames {
            rx,
            buf: String::new(),
            th: Some(th),
        })
    }
    /// Next (event, data) within 10 s.
    fn next(&mut self) -> Option<(String, Value)> {
        loop {
            if let Some(i) = self.buf.find("\n\n") {
                let frame: String = self.buf.drain(..i + 2).collect();
                let mut ev = String::new();
                let mut data = Value::Null;
                for l in frame.lines() {
                    if let Some(x) = l.strip_prefix("event: ") {
                        ev = x.into();
                    } else if let Some(x) = l.strip_prefix("data: ") {
                        data = serde_json::from_str(x).unwrap();
                    }
                }
                return Some((ev, data));
            }
            let chunk = self
                .rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .ok()?;
            self.buf.push_str(&String::from_utf8_lossy(&chunk));
        }
    }
    fn until(&mut self, want: &str) -> Value {
        loop {
            let (ev, d) = self.next().unwrap_or_else(|| panic!("no {want} frame"));
            if ev == want {
                return d;
            }
            assert_ne!(ev, "bye", "unexpected bye while waiting for {want}: {d}");
        }
    }
    fn join(mut self) {
        self.th.take().unwrap().join().unwrap();
    }
}

fn answer(e: &Enroll, token: &str, body: &Value) -> (u16, Value) {
    let mut r = post("/openmind/node/tunnel/answer", body);
    r.headers
        .push(("authorization".into(), format!("Bearer {token}")));
    status_body(e.handle(&r, false, crate::time::now_secs()))
}

#[test]
fn tunnel_hello_proof_ping_replace_and_revoke() {
    let dir = tmp("tunnel");
    let now = crate::time::now_secs();
    let e = Arc::new(
        Enroll::open(EnrollCfg {
            url: URL.into(),
            ..cfg(&dir)
        })
        .unwrap(),
    );
    let d = Dev::new(47);
    let tok = enrolled_at(&e, &d, 1, now);
    // no / wrong credential: 401, no stream
    assert_eq!(Frames::open(&e, "omn1_nope").err().unwrap().0, 401);
    // open: hello with a challenge
    let mut f = Frames::open(&e, &tok).ok().unwrap();
    let hello = f.until("hello");
    let session = hello["session"].as_str().unwrap().to_string();
    let ch = hello["challenge"].as_str().unwrap().to_string();
    assert_eq!(hello["device_id"], json!(d.id()));
    // owner ping before the proof: refused
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/ping", &json!({"device_id": d.id()})),
        true,
        now,
    ));
    assert_eq!(s, 409);
    // a wrong-key answer is refused; the right one verifies; a replay is refused
    let other = Dev::new(48);
    let (s, _) = answer(&e, &tok, &other.answer(&session, "hello", &ch));
    assert_eq!(s, 401);
    // the bad hello answer closed this tunnel; reconnect
    assert_eq!(f.until_bye(), "hello answer did not verify");
    f.join();
    let mut f = Frames::open(&e, &tok).ok().unwrap();
    let hello = f.until("hello");
    let session = hello["session"].as_str().unwrap().to_string();
    let ch = hello["challenge"].as_str().unwrap().to_string();
    let (s, b) = answer(&e, &tok, &d.answer(&session, "hello", &ch));
    assert_eq!(s, 200, "{b}");
    assert_eq!(answer(&e, &tok, &d.answer(&session, "hello", &ch)).0, 409);
    assert_eq!(f.until("ready")["verified"], json!(true));
    assert!(f.until("ping")["n"].as_u64().unwrap() >= 1);
    // owner sees the tunnel
    let (_, b) = status_body(e.handle(&get("/openmind/enroll/devices", "", None), true, now));
    assert_eq!(b["devices"][0]["tunnel"]["connected"], json!(true));
    assert_eq!(b["devices"][0]["tunnel"]["verified"], json!(true));
    // owner ping: challenge goes down the stream, the device signs it
    let e2 = e.clone();
    let id = d.id();
    let pinger = std::thread::spawn(move || {
        status_body(e2.handle(
            &post("/openmind/enroll/ping", &json!({"device_id": id})),
            true,
            crate::time::now_secs(),
        ))
    });
    let c = f.until("challenge");
    let (s, _) = answer(
        &e,
        &tok,
        &d.answer(
            &session,
            c["id"].as_str().unwrap(),
            c["challenge"].as_str().unwrap(),
        ),
    );
    assert_eq!(s, 200);
    let (s, b) = pinger.join().unwrap();
    assert_eq!(s, 200, "{b}");
    assert!(b["rtt_ms"].as_u64().is_some());
    // a second tunnel from the same device replaces the first
    let mut f2 = Frames::open(&e, &tok).ok().unwrap();
    f2.until("hello");
    assert_eq!(f.until_bye(), "replaced by a newer tunnel");
    f.join();
    // max_tunnels = 2 in cfg(): a third device is refused while two are open
    let d3 = Dev::new(49);
    let d4 = Dev::new(50);
    let t3 = enrolled_at(&e, &d3, 10, now);
    let t4 = enrolled_at(&e, &d4, 20, now);
    let mut f3 = Frames::open(&e, &t3).ok().unwrap();
    f3.until("hello");
    assert_eq!(Frames::open(&e, &t4).err().unwrap().0, 503);
    // revoke: the open stream says bye at once, the credential is dead
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/revoke", &json!({"device_id": d.id()})),
        true,
        now,
    ));
    assert_eq!(s, 200);
    assert_eq!(f2.until_bye(), "credential revoked or expired");
    f2.join();
    assert_eq!(Frames::open(&e, &tok).err().unwrap().0, 401);
    // the hello deadline is enforced (checked without waiting 30 s: the
    // constant is what the loop compares against)
    assert_eq!(PROOF_DEADLINE, 30);
    let (s, _) = status_body(e.handle(
        &post("/openmind/enroll/revoke", &json!({"device_id": d3.id()})),
        true,
        now,
    ));
    assert_eq!(s, 200);
    assert_eq!(f3.until_bye(), "credential revoked or expired");
    f3.join();
    std::fs::remove_dir_all(&dir).ok();
}

impl Frames {
    fn until_bye(&mut self) -> String {
        loop {
            let (ev, d) = self.next().expect("no bye frame");
            if ev == "bye" {
                return d["reason"].as_str().unwrap().to_string();
            }
        }
    }
}

/// [`enrolled`] at a real clock time (the tunnel checks credentials with
/// the wall clock).
fn enrolled_at(e: &Enroll, d: &Dev, base: u8, now: i64) -> String {
    let (s, _) = status_body(e.handle(
        &post(ENROLL_PATH, &d.doc(&n(base), now, &["cpu"])),
        false,
        now,
    ));
    assert_eq!(s, 202);
    let (s, _) = status_body(e.handle(
        &post(
            "/openmind/enroll/approve",
            &json!({"code": d.code(&n(base))}),
        ),
        true,
        now,
    ));
    assert_eq!(s, 200);
    let (s, b) = status_body(e.handle(
        &post("/openmind/enroll/claim", &d.claim(&n(base + 1), now)),
        false,
        now,
    ));
    assert_eq!(s, 200, "{b}");
    b["token"].as_str().unwrap().to_string()
}
