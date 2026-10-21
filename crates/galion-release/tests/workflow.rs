// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! release.yml stays locked and keeps its packaging in galion-release
//! (S4-T7). Plain text checks on the workflow file; no YAML parser needed.

fn workflow() -> String {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/release.yml"
    );
    std::fs::read_to_string(p).expect("release.yml")
}

fn on_block(w: &str) -> String {
    let start = w.find("\non:\n").expect("on: block");
    let rest = &w[start + 5..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn trigger_is_v_tags_only() {
    let w = workflow();
    let on = on_block(&w);
    assert!(
        on.contains("push:") && on.contains("tags: [ 'v*' ]"),
        "{on}"
    );
    for bad in [
        "branches",
        "pull_request",
        "workflow_dispatch",
        "schedule",
        "release:",
    ] {
        assert!(!on.contains(bad), "release.yml on: has {bad}");
    }
}

#[test]
fn release_job_stays_gated() {
    let w = workflow();
    assert!(w.contains("needs: self-eval"));
    assert!(w.contains(
        "if: github.event.repository.private == false && vars.RELEASE_ENABLED == 'true' && startsWith(github.ref, 'refs/tags/v')"
    ));
    // Top-level token stays read-only; write scopes only inside the gated job.
    assert!(w.contains("permissions:\n  contents: read\n"));
}

#[test]
fn packages_both_tier1_targets_with_galion_release() {
    let w = workflow();
    assert!(w.contains("UCPU_TIER1: x86_64-unknown-linux-musl aarch64-unknown-linux-musl"));
    assert!(w.contains("targets: x86_64-unknown-linux-musl, aarch64-unknown-linux-musl"));
    assert!(w.contains("scripts/ucpu-gate.sh build static size"));
    assert!(w.contains("galion-release package --target \"$t\""));
    assert!(w.contains("--bin-dir \"target/omnode/$t/release\""));
    assert!(w.contains("galion-release sums dist"));
    assert!(w.contains("sha256sum -c SHA256SUMS"));
    assert!(w.contains("galion-release rehearse dist"));
    // S5-T3: provenance stamps are checked against this build (binding).
    assert!(w.contains("--expect-commit \"$GALION_BUILD_COMMIT\""));
    assert!(w.contains("--expect-built \"$GALION_BUILD_TIME\""));
    assert!(w.contains("--help-timeout 5"));
    assert!(w.contains("qemu-user-static"));
    // No hand-picked binary list and no shell tar packaging any more.
    assert!(!w.contains("for b in galion-one"));
    assert!(!w.contains("tar -C dist"));
    // SHA256SUMS is written after the SBOM, so the SBOM is covered too.
    let sbom = w.find("anchore/sbom-action").unwrap();
    let sums = w.find("galion-release sums dist").unwrap();
    let sign = w.find("cosign sign-blob").unwrap();
    assert!(sbom < sums && sums < sign);
}

#[test]
fn every_action_is_pinned_by_sha() {
    for l in workflow().lines() {
        if let Some(u) = l
            .trim()
            .strip_prefix("- uses: ")
            .or(l.trim().strip_prefix("uses: "))
        {
            if u.starts_with("./") {
                continue;
            }
            let (_, rev) = u.split_once('@').expect("pinned");
            let sha = rev.split_whitespace().next().unwrap();
            assert_eq!(sha.len(), 40, "{l}");
            assert!(sha.bytes().all(|b| b.is_ascii_hexdigit()), "{l}");
        }
    }
}
