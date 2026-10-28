// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 423 (D-383-2 b): `GAIA_TEST_PORTS` is a real-test-only override of
//! the box test range. It is parsed and refused here with explicit inputs
//! (`test_ports_from`, `check_target_in`), so no test touches the process
//! environment.

use gaia_live::heal::*;

fn prog() -> String {
    for p in ["/bin/true", "/usr/bin/true", "/bin/echo", "/usr/bin/echo"] {
        if std::path::Path::new(p).is_file() {
            return p.into();
        }
    }
    panic!("no /bin/true or /bin/echo")
}

fn target(port: u16) -> TestTarget {
    TestTarget {
        name: "t423".into(),
        port,
        health_path: "/health".into(),
        argv: vec![prog(), "--port".into(), port.to_string()],
    }
}

#[test]
fn unset_or_empty_is_the_production_default() {
    assert_eq!(test_ports_from(None, false), Ok(TEST_PORTS));
    assert_eq!(test_ports_from(None, true), Ok(TEST_PORTS));
    assert_eq!(test_ports_from(Some(""), false), Ok(TEST_PORTS));
    assert_eq!(test_ports_from(Some("  "), true), Ok(TEST_PORTS));
    assert_eq!(TEST_PORTS, 19000..=19999, "production guard unchanged");
}

#[test]
fn override_needs_galion_real() {
    let e = test_ports_from(Some("22930-22939"), false).unwrap_err();
    assert!(e.contains("GALION_REAL=1"), "{e}");
    // Even a range inside the default is refused without GALION_REAL.
    assert!(test_ports_from(Some("19000-19009"), false).is_err());
}

#[test]
fn override_parses_inside_bounds() {
    assert_eq!(
        test_ports_from(Some("22930-22939"), true),
        Ok(22930..=22939)
    );
    assert_eq!(
        test_ports_from(Some(" 19000-22999 "), true),
        Ok(19000..=22999)
    );
    assert_eq!(test_ports_from(Some("22931"), true), Ok(22931..=22931));
    assert_eq!(
        test_ports_from(Some("19000-19009"), true),
        Ok(19000..=19009)
    );
}

#[test]
fn override_refuses_bad_and_out_of_bounds() {
    for bad in [
        "22939-22930", // reversed (empty)
        "18999-19009", // starts below
        "22990-23000", // ends above
        "23000-23009",
        "8670",      // live nerve
        "8000-8999", // live stack
        "1-65535",
        "0",
        "abc",
        "22930-",
        "-22930",
        "22930-22939-22940",
        "65536",
    ] {
        assert!(
            test_ports_from(Some(bad), true).is_err(),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn check_target_in_follows_the_override() {
    let r = test_ports_from(Some("22930-22939"), true).unwrap();
    let t = target(22931);
    check_target_in(&t, &(22930..=22939), &r).unwrap();
    // The same target is refused under the production default.
    let e = check_target_in(&t, &(22930..=22939), &TEST_PORTS).unwrap_err();
    assert!(e.contains("outside the test range 19000-19999"), "{e}");
    // An allowed range wider than the override is refused.
    assert!(check_target_in(&t, &(22920..=22939), &r).is_err());
    // A default-range target is refused under the override (it replaces).
    assert!(check_target_in(&target(19005), &(19000..=19009), &r).is_err());
}

#[test]
fn check_target_in_never_widens_past_bounds() {
    // A caller handing a wide range straight to check_target_in is refused.
    for wide in [
        1..=65535u16,
        8000..=19999,
        19000..=23000,
        std::ops::RangeInclusive::new(22999, 22990),
    ] {
        let e = check_target_in(&target(19005), &(19000..=19009), &wide).unwrap_err();
        assert!(e.contains("override bounds 19000-22999"), "{wide:?}: {e}");
    }
    // Live stack ports stay refused whatever the test range.
    let r = test_ports_from(Some("19000-22999"), true).unwrap();
    for live in [8080u16, 8091, 8420, 8642, 8650, 8670] {
        assert!(check_target_in(&target(live), &(live..=live), &r).is_err());
    }
}

#[test]
fn production_check_target_is_default_without_env() {
    // The test harness does not set GAIA_TEST_PORTS; check_target equals
    // the default guard (skip if a caller exported it on purpose).
    if std::env::var_os(TEST_PORTS_ENV).is_some() {
        eprintln!("skipped: {TEST_PORTS_ENV} set in the environment");
        return;
    }
    check_target(&target(19005), &(19000..=19009)).unwrap();
    assert!(check_target(&target(22931), &(22930..=22939)).is_err());
}
