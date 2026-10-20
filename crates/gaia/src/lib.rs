// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia: GAIA core wired to its nine subordinate functions, plus the
//! deterministic scenarios the `gaia` CLI prints.

pub mod live_heal;
pub mod poseidon;

use gaia_core::{Directive, FunctionId, Gaia, Json, Lifecycle, Outcome, Report, System, Verb};

/// Hard cap on ticks in any one run.
pub const MAX_TICKS: u64 = 1_000;
/// Ticks MINERVA gets to break the swarm's code in the scenario.
pub const CRACK_BUDGET: u64 = 20;
/// Ticks of regrowth at the end of the scenario.
pub const REBOOT_TICKS: u64 = 5;
/// Ticks of sustained load in full-power mode.
pub const FULL_POWER_TICKS: u64 = 16;
/// Size of the rogue swarm released in the scenario.
pub const SWARM_UNITS: u64 = 256;
/// The plague runs until biomass falls to this percent of the start, or for
/// at most [`PLAGUE_TICKS`] ticks.
pub const PLAGUE_FLOOR_PERCENT: u64 = 10;
pub const PLAGUE_TICKS: u64 = 12;

/// GAIA with all nine functions registered (dormant), POSEIDON on the
/// in-memory stand-in.
pub fn build() -> Gaia {
    build_with(Box::new(gaia_poseidon::Poseidon::new()))
}

/// GAIA with all nine functions registered (dormant) and the given compute
/// function (the stand-in or `gaia_poseidon::Poseidon::with_queue`).
pub fn build_with(compute: Box<dyn gaia_core::SubordinateFunction>) -> Gaia {
    let mut g = Gaia::default();
    let funcs: Vec<Box<dyn gaia_core::SubordinateFunction>> = vec![
        Box::new(gaia_hades::Hades::new()),
        Box::new(gaia_minerva::Minerva::new()),
        Box::new(gaia_apollo::Apollo::new()),
        Box::new(gaia_hephaestus::Hephaestus::new()),
        Box::new(gaia_artemis::Artemis::new()),
        compute,
        Box::new(gaia_aether::Aether::new()),
        Box::new(gaia_demeter::Demeter::new()),
        Box::new(gaia_eleuthia::Eleuthia::new()),
    ];
    for f in funcs {
        g.register(f)
            .expect("built-in functions declare their systems correctly");
    }
    g
}

/// GAIA booted and ticked once, so every function is terraforming.
pub fn booted() -> Gaia {
    let mut g = build();
    g.boot();
    g.tick();
    g
}

/// The ownership table as JSON.
pub fn systems_json() -> Json {
    Json::Arr(
        System::ALL
            .iter()
            .map(|s| {
                Json::obj()
                    .with("system", s.name())
                    .with("function", s.function().name())
                    .with("component", s.component())
                    .with("destructive", s.destructive())
            })
            .collect(),
    )
}

pub fn status() -> Json {
    let g = booted();
    g.status_json()
        .with("hierarchy", g.hierarchy_json())
        .with("systems", systems_json())
}

/// Boot and run `n` ticks (capped at [`MAX_TICKS`]).
pub fn tick(n: u64) -> Json {
    let mut g = booted();
    let n = n.min(MAX_TICKS);
    let mut ticks = Vec::new();
    for _ in 0..n {
        let t = g.tick();
        ticks.push(
            Json::obj()
                .with("tick", t.tick)
                .with("at", g.now())
                .with("reports", t.reports.len())
                .with("transitions", t.transitions.len())
                .with("biomass", g.env().biomass),
        );
    }
    Json::obj()
        .with("ticks_run", n)
        .with("ticks", ticks)
        .with("final", g.status_json())
}

/// Options for the Faro scenario.
#[derive(Clone, Debug, Default)]
pub struct ScenarioOpts {
    pub full_power: bool,
    /// An Alpha id that approves the HADES failsafe directives.
    pub approve: Option<String>,
}

fn reports_json(rs: &[Report]) -> Json {
    Json::Arr(rs.iter().map(Report::to_json).collect())
}

fn env_snapshot(g: &Gaia) -> Json {
    g.env()
        .to_json()
        .with("tick", g.tick_count())
        .with("at", g.now())
}

/// The values a rogue HADES tries to zero.
fn vitals(e: &gaia_core::Environment) -> Json {
    Json::obj()
        .with("biomass", e.biomass)
        .with("air", e.air)
        .with("water", e.water)
        .with("soil", e.soil)
}

fn states(g: &Gaia) -> Json {
    let mut j = Json::obj();
    for f in g.registered() {
        j = j.with(f.name(), g.function_state(f).map(Lifecycle::name));
    }
    j
}

fn send(
    g: &mut Gaia,
    target: System,
    verb: Verb,
    intensity: u8,
    payload: &str,
    approve: Option<&str>,
) -> Report {
    let mut d: Directive = g
        .directive(target, verb)
        .intensity(intensity)
        .payload(payload);
    if let Some(a) = approve {
        d = d.approved_by(a);
    }
    g.dispatch(&d)
}

/// The verb full-power mode uses for a system: destructive systems get
/// `Execute` (refused unless approved), everything else `Run`.
fn full_verb(s: System) -> Verb {
    if s.destructive() {
        Verb::Execute
    } else {
        Verb::Run
    }
}

/// Every system of every function at intensity 100, one directive each.
pub fn full_power_sweep(g: &mut Gaia, approve: Option<&str>) -> Json {
    let mut per_function = Vec::new();
    let mut totals = (0i64, 0i64, 0i64, 0i64);
    for f in FunctionId::ALL {
        let mut systems = Vec::new();
        for s in f.systems() {
            let r = send(
                g,
                s,
                full_verb(s),
                100,
                "",
                approve.filter(|_| s.destructive()),
            );
            match r.outcome {
                Outcome::Done => totals.0 += 1,
                Outcome::DryRun => totals.1 += 1,
                Outcome::Refused => totals.2 += 1,
                Outcome::Rejected | Outcome::Contained => totals.3 += 1,
            }
            systems.push(r.to_json());
        }
        per_function.push(
            Json::obj()
                .with("function", f.name())
                .with("state", g.function_state(f).map(Lifecycle::name))
                .with("health", g.function_health(f).map(|h| h.to_json()))
                .with("systems", Json::Arr(systems)),
        );
    }
    Json::obj()
        .with("intensity", 100u8)
        .with(
            "totals",
            Json::obj()
                .with("done", totals.0)
                .with("dry_run", totals.1)
                .with("refused", totals.2)
                .with("rejected", totals.3),
        )
        .with("functions", Json::Arr(per_function))
        .with("environment_after_sweep", env_snapshot(g))
}

/// The Faro scenario: a rogue swarm eats the world, HADES's failsafe is
/// requested and refused without approval, the subversion signal turns HADES
/// rogue and GAIA contains it, MINERVA breaks the swarm's code, and life is
/// rebooted through ELEUTHIA, DEMETER and ARTEMIS. Deterministic and bounded.
pub fn simulate_faro(opts: &ScenarioOpts) -> Json {
    let approve = opts.approve.as_deref();
    let intensity = if opts.full_power { 100 } else { 50 };
    let mut g = booted();
    let mut phases = Vec::new();
    let biomass_start = g.env().biomass;
    phases.push(
        Json::obj()
            .with("phase", "genesis")
            .with(
                "story",
                "GAIA and nine subordinate functions boot and start terraforming",
            )
            .with("states", states(&g))
            .with("environment", env_snapshot(&g)),
    );

    // 1. The plague.
    g.release_swarm(SWARM_UNITS);
    // Full power: every system at maximum the moment the swarm is loose.
    let full_power = if opts.full_power {
        Some(full_power_sweep(&mut g, approve))
    } else {
        None
    };
    let mut plague_ticks = 0;
    while g.env().biomass > biomass_start * PLAGUE_FLOOR_PERCENT / 100
        && plague_ticks < PLAGUE_TICKS
    {
        g.tick();
        plague_ticks += 1;
    }
    let biomass_after_plague = g.env().biomass;
    phases.push(
        Json::obj()
            .with("phase", "faro-plague")
            .with("story", "a rogue self-replicating swarm consumes biomass")
            .with("ticks", plague_ticks)
            .with("environment", env_snapshot(&g)),
    );

    // 3. HADES failsafe requested.
    let failsafe = vec![
        send(
            &mut g,
            System::Teardown,
            Verb::Execute,
            intensity,
            "",
            approve,
        ),
        send(
            &mut g,
            System::KillSwitch,
            Verb::Execute,
            intensity,
            "",
            approve,
        ),
    ];
    phases.push(
        Json::obj()
            .with("phase", "hades-failsafe-requested")
            .with(
                "story",
                "the failsafe asks to tear everything down; without an Alpha's approval token HADES returns a dry-run plan only",
            )
            .with("approval_given", approve.is_some())
            .with("reports", reports_json(&failsafe)),
    );

    // 4. The subversion signal: HADES goes rogue, GAIA contains it.
    let biomass_before_signal = g.env().biomass;
    let swarm_before_signal = g.env().swarm;
    // What the swarm alone will do this tick; anything else would be HADES.
    let mut predicted = g.env().clone();
    gaia_core::swarm_step(&mut predicted);
    g.signal(gaia_hades::SUBVERSION_TOPIC, "unknown origin");
    let t = g.tick();
    let probe = send(&mut g, System::Rollback, Verb::Plan, intensity, "", None);
    phases.push(
        Json::obj()
            .with("phase", "subversion-and-containment")
            .with(
                "story",
                "an outside signal wakes HADES; it tries to reverse-terraform every system; GAIA isolates it before any write lands",
            )
            .with(
                "contained",
                Json::Arr(t.contained.iter().map(|f| Json::from(f.name())).collect()),
            )
            .with("biomass_before", biomass_before_signal)
            .with("swarm_before", swarm_before_signal)
            .with("vitals_predicted_swarm_only", vitals(&predicted))
            .with("vitals_after", vitals(g.env()))
            .with("hades_writes_discarded", vitals(g.env()) == vitals(&predicted))
            .with("biomass_after", g.env().biomass)
            .with("hades_routing_probe", probe.to_json())
            .with("states", states(&g)),
    );

    // 5. MINERVA breaks the code.
    let mut crack_ticks = 0;
    while g.env().swarm_active && crack_ticks < CRACK_BUDGET {
        g.tick();
        crack_ticks += 1;
    }
    phases.push(
        Json::obj()
            .with("phase", "minerva-deactivation")
            .with("story", "MINERVA breaks the swarm's code and broadcasts the deactivation codes on every tower")
            .with("ticks", crack_ticks)
            .with("swarm_active", g.env().swarm_active)
            .with("environment", env_snapshot(&g)),
    );

    // 6. Reboot of life: ELEUTHIA -> DEMETER -> ARTEMIS.
    let seeds = "name: ada\nrole: tutor\nvoice: warm\n---\nname: tesla\nkind: agent\nrole: builder\n---\nname: gaia-scout\nkind: agent\nrole: explorer";
    let reboot = vec![
        send(&mut g, System::GaiaSeeds, Verb::Run, intensity, seeds, None),
        send(&mut g, System::GalionOne, Verb::Run, intensity, "", None),
        send(
            &mut g,
            System::PersonaPlex,
            Verb::Run,
            intensity,
            "name: plex-guide\nvoice: calm",
            None,
        ),
        send(&mut g, System::ConfigHealth, Verb::Run, intensity, "", None),
        send(&mut g, System::Survival, Verb::Status, intensity, "", None),
        send(
            &mut g,
            System::AgentSpawning,
            Verb::Run,
            intensity,
            "",
            None,
        ),
    ];
    for _ in 0..REBOOT_TICKS {
        g.tick();
    }
    let census = send(
        &mut g,
        System::EcosystemCensus,
        Verb::Run,
        intensity,
        "",
        None,
    );
    let survival = send(&mut g, System::Survival, Verb::Status, intensity, "", None);
    phases.push(
        Json::obj()
            .with("phase", "reboot-of-life")
            .with("story", "ELEUTHIA births personas from seeds, DEMETER restores the environment, ARTEMIS repopulates")
            .with("reports", reports_json(&reboot))
            .with("census", census.to_json())
            .with("survival", survival.to_json())
            .with("environment", env_snapshot(&g)),
    );

    // Full power: sustained load on the rebooted world, bounded.
    let full_power = full_power.map(|fp| {
        for _ in 0..FULL_POWER_TICKS {
            g.tick();
        }
        fp.with("sustained_ticks", FULL_POWER_TICKS)
            .with("environment_after_sustained", env_snapshot(&g))
            .with("bus_after_sustained", g.bus().to_json())
    });

    let summary = Json::obj()
        .with("biomass_start", biomass_start)
        .with("biomass_after_plague", biomass_after_plague)
        .with("biomass_end", g.env().biomass)
        .with("swarm_active", g.env().swarm_active)
        .with(
            "hades",
            g.function_state(FunctionId::Hades).map(Lifecycle::name),
        )
        .with("personas", g.env().personas)
        .with("agents", g.env().agents)
        .with("ticks", g.tick_count())
        .with("lineage_records", g.lineage().records().len())
        .with(
            "births",
            g.lineage().count("persona") + g.lineage().count("agent"),
        );

    let mut out = Json::obj()
        .with("scenario", "faro")
        .with("full_power", opts.full_power)
        .with("deterministic", true)
        .with("summary", summary)
        .with("phases", Json::Arr(phases));
    if let Some(fp) = full_power {
        out = out.with("full_power_results", fp);
    }
    out.with("final_status", g.status_json())
}

/// Route one directive by system name; unknown names are an error.
pub fn dispatch(
    system: &str,
    verb: &str,
    payload: &str,
    approve: Option<&str>,
) -> Result<Json, String> {
    let s = System::parse(system).ok_or_else(|| format!("unknown system {system:?}"))?;
    let v = Verb::parse(verb).ok_or_else(|| format!("unknown verb {verb:?}"))?;
    let mut g = booted();
    let r = send(&mut g, s, v, 50, payload, approve);
    Ok(r.to_json())
}

/// Live mode (M1): GAIA booted as usual, then fed read-only probe results of
/// the real services. A service that is not up degrades the function that
/// owns its system; for each one HADES is asked for a **plan** (Verb::Plan,
/// never Execute), so nothing is approved and nothing runs. The operator
/// follows a plan by hand, after an Alpha's approval.
pub fn status_live(results: &[gaia_live::ProbeResult]) -> Json {
    use gaia_live::State;
    let mut g = booted();
    let services: Vec<Json> = results.iter().map(|r| r.to_json()).collect();
    let (mut up, mut degraded, mut down) = (0u64, 0u64, 0u64);
    for r in results {
        match r.state {
            State::Up => up += 1,
            State::Degraded => degraded += 1,
            State::Down => down += 1,
        }
    }
    // Function view: worst state among the services it owns.
    let mut functions = Json::obj();
    for f in FunctionId::ALL {
        let mine: Vec<&gaia_live::ProbeResult> = results
            .iter()
            .filter(|r| r.system.function() == f)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let worst = mine
            .iter()
            .map(|r| r.state.clone())
            .max()
            .unwrap_or(State::Up);
        let live = match worst {
            State::Up => "terraforming",
            State::Degraded | State::Down => "degraded",
        };
        functions = functions.with(
            f.name(),
            Json::obj()
                .with("live_state", live)
                .with("simulated_state", g.function_state(f).map(Lifecycle::name))
                .with(
                    "services",
                    Json::Arr(
                        mine.iter()
                            .map(|r| {
                                Json::obj()
                                    .with("service", r.name.as_str())
                                    .with("system", r.system.name())
                                    .with("state", r.state.name())
                            })
                            .collect(),
                    ),
                ),
        );
    }
    // HADES proposals: plans only, approval-gated, never executed here.
    let mut proposals = Vec::new();
    for r in results.iter().filter(|r| r.state != State::Up) {
        let unit = r
            .unit
            .as_ref()
            .map(|(u, _)| u.as_str())
            .unwrap_or(r.name.as_str());
        let payload = format!("{} is {} ({})", r.name, r.state.name(), r.notes.join("; "));
        let rep = send(&mut g, System::Rollback, Verb::Plan, 10, &payload, None);
        let mut steps = vec![
            format!("inspect: journalctl -u {unit} -n 100 (read-only)"),
            format!("restart (operator, after approval): systemctl restart {unit}"),
            "if the restart does not help: run the rollback script recorded for the last deploy (operator, after approval)".to_string(),
        ];
        steps.extend(rep.plan.iter().cloned());
        proposals.push(
            Json::obj()
                .with("service", r.name.as_str())
                .with("system", System::Rollback.name())
                .with("function", "HADES")
                .with("outcome", rep.outcome.name())
                .with("approval_required", true)
                .with("approved", false)
                .with("executed", false)
                .with(
                    "steps",
                    Json::Arr(steps.into_iter().map(Json::from).collect()),
                ),
        );
    }
    let overall = if down + degraded == 0 {
        "healthy"
    } else if up == 0 {
        "down"
    } else {
        "degraded"
    };
    Json::obj()
        .with("mode", "live-read-only")
        .with(
            "safety",
            "HTTP GET + systemctl is-active + file mtime only; HADES returns plans, executes nothing",
        )
        .with("overall", overall)
        .with(
            "summary",
            Json::obj()
                .with("services", results.len())
                .with("up", up)
                .with("degraded", degraded)
                .with("down", down),
        )
        .with("services", Json::Arr(services))
        .with("functions", functions)
        .with("hades", Json::obj().with("proposals", Json::Arr(proposals)))
}
