<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# integration-verify: main-merge blocker checks on swarm/integration-2 df42e0f

Wave 4 slot 111 (`swarm/integration-verify`), run 2026-10-21 02:07-02:30 CEST (+02:00) on the box.
Scope = plan `gaia.md` S1.4 (c) and the rest of (d), which `docs/merges/integration-2.md` left open:
the 7 older real-life tests, REUSE lint and `cargo deny`. Code under test: `df42e0f` unchanged
(this branch adds only this file).

How it was run: every cargo build went through the wave-4 build queue
(`cargo-slot ... -j 2`, `CARGO_TARGET_DIR=/workspace/swarm/target/integration-verify`).
The release binaries each script needs were pre-built that way first, using each script's exact
`cargo build` arguments, so each script's own `cargo build` found them up to date and built nothing.
The scripts ran unmodified, with `CARGO_TARGET_DIR` exported. They use their original slot ports
(19020, 19110-19114, 19120-19122, 19230-19232, 19240/19249). All of these were free before the run
(`ss -ltnu`) and closed afterwards (each script checks this itself). No Python was used.

## 1. The 7 real-life tests (S1.4 c)

| # | test | how | start / end (CEST +02:00) | result |
|---|---|---|---|---|
| 1 | gaia-live | `scripts/real/gaia-live.sh` (superseded wrapper that execs gaia-hades-tokens.sh) | 02:19:03 / 02:19:06 | PASS, rc 0 |
| 2 | gaia-hades-tokens | `scripts/real/gaia-hades-tokens.sh` | 02:19:06 / 02:19:08 | PASS, rc 0: all 16 tick cases gave the expected outcome (healthy; live-port refused; no-token, forged, tampered, wrong-service/action/plan, expired, admin-safety, disabled refused; 3 valid restarts 1/3..3/3; replay refused; 4th capped); no token or seed leaked; ports 19230-19232 closed |
| 3 | gaia-poseidon | `scripts/real/gaia-poseidon.sh` | 02:19:08 / 02:19:09 | PASS, `RESULT pass=60 fail=0` |
| 4 | hades-approval | `scripts/real/hades-approval.sh` (x86_64 musl hades-gate) | 02:19:09 / 02:19:12 | PASS, `result: ALL PASS` (21 PASS lines, port 19020 closed) |
| 5 | poseidon | `scripts/real/poseidon.sh` (incl. `GALION_REAL=1` `openmind-queue --test real_binary` SIGKILL/restart) | 02:19:12 / 02:19:21 | PASS, `RESULT pass=51 fail=0` |
| 6 | lan-discovery | `scripts/real/lan-discovery.sh` (mDNS on 19110/udp, probe, approve/deny/join ledger) | 02:19:21 / 02:19:41 | PASS, rc 0 |
| 7 | gaia-land | Rust: `GALION_REAL=1 cargo-slot test --release -p gaia --test real_cli -- --nocapture --test-threads 1` (the .sh was already ported) | 02:19:41 / 02:19:42 (fixtures); 02:25:14 / 02:25:42 with `GAIA_VPS` set | PASS, 2 passed / 0 failed both times |

gaia-land live part (READ-ONLY, fixed `systemctl list-units` and `ss -ltnp` listings over ssh; nothing changed on the VPS):
`units_seen 16, units_running 12, units_unmapped 0, listeners 22, listeners_unmapped 0,
listeners_not_declared_by_a_unit 5, systems_total 52, systems_live 10, systems_with_unit 11, systems_with_crate 28`.
Fixture view: the same numbers as in `docs/real-tests/gaia-land.md`, except `systems_with_crate` is now 28 (it was 27).
That rise comes from the crates integration-2 merged in.

## 2. REUSE / cargo deny / public export (S1.4 d)

Run: `GALION_TOOLS_BIN=/workspace/swarm/tools/bin scripts/public-export.sh --ref df42e0f --out <scratch> --allow-missing-tools --online`
(02:19:24 - 02:19:28 CEST +02:00). The tools were installed into `/workspace/swarm/tools/bin` with
`cargo-slot install --locked`. They are not repo dependencies: `cargo-deny 0.20.2` (MIT OR Apache-2.0,
https://github.com/EmbarkStudios/cargo-deny) and `comply 0.1.1` (MIT OR Apache-2.0, REUSE lint in Rust,
https://crates.io/crates/comply). Neither gitleaks nor cargo-cyclonedx was installed, so gates 6 and 9 were SKIPPED.

| gate | result | detail |
|---|---|---|
| 1 git archive | PASS | 1110 of 1202 tracked files exported (92 export-ignored) |
| 2 path policy | PASS | |
| 3 no binaries | PASS | |
| 4 no Python | PASS | 0 `*.py` |
| 5 secret patterns | PASS | 0 hits over 14 token shapes |
| 6 gitleaks | SKIP | not installed |
| 7 REUSE lint (comply) | **PASS** | 1093 total, 1036 passed, 0 failed, 57 skipped. The 57 skipped are all `LICENSE` files (root, 53 crates, edge/openmind-node, release/*, stack/agents), which REUSE exempts. |
| 8 cargo deny, licences | **PASS** | workspace + edge/openmind-node: `licenses ok` |
| 8 cargo deny, bans | **PASS** | `bans ok` (warnings only: duplicate syn x2 and hashbrown x3; path wildcards in crates without `publish = false`) |
| 8 cargo deny, sources | **PASS** | `sources ok` |
| 8 cargo deny, advisories | **FAIL** (workspace only; edge/openmind-node ok) | see below |
| 9 SBOM | SKIP | cargo-cyclonedx not installed |
| 10 infra disclosure | **WARN** | 22 public IPv4 literals, 1 quick-tunnel hostname; see below |

### Advisories (workspace `Cargo.lock`)

- `zip 7.4.0`: RUSTSEC-2026-0367, -0368 and -0369 (vulnerabilities; fixed in >= 9.0.0). The only path is
  `candle-core 0.9.2` -> `candle-nn` -> `galion-voice-cpu`. Wave 4 slot 109 (`candle-zip-fix`) is on this.
- `paste 1.0.15`: RUSTSEC-2024-0436 (unmaintained). It comes in via `gemm-common 0.19.0`, which is also in the candle tree.
  A candle bump may or may not remove it. If it does not, the options are an advisory `ignore` entry in `deny.toml`
  (which needs the owner's approval, per the file's own rule) or dropping or replacing the dependency.

### Infra disclosure (addresses are not repeated here)

The 22 IPv4 hits are mostly public resolvers in test fixtures (8.8.8.8, 8.8.8.0, 1.1.1.1) in
openmind-discovery, openmind-nerve, repo-check and hermes-tools tests and docs.
**One hit is the production VPS address in `docs/real-tests/survival-heal.md` line 21.** It came in with
survival-heal commit 45a18aa and is not on main 13a296c. If integration-2 or integration-3 is merged
to main as it stands, that address lands in main's history. The quick-tunnel hostname is in
`docs/session-2026-10-18-evening.md`.

## 3. Verdict for the main merge (S1.4 c + d on df42e0f)

- Real tests: 7/7 PASS.
- REUSE: PASS. cargo deny licences, bans and sources: PASS.
- cargo deny advisories: FAIL (zip via candle and paste via gemm). This is the remaining automated blocker.
- Disclosure: WARN. Redact the VPS address in `docs/real-tests/survival-heal.md` before the merge (owner decides on the rest).
- Not covered here: gitleaks and SBOM (tools not installed); S1.4 a/b were done in the integration-2 merge report.
