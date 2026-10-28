// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Staged chmod plan (slot 379, D-356-3): owner-aware stat lists, plan build,
//! TSV round trip, rollback, guarded sh output and verification.

use repo_check::perms::*;

/// Shape of the live VPS listing on 2026-10-28 10:08 CET (metadata only,
/// `find -printf '%m %y %u %g %p'`).
const VPS: &str = "755 d root root /opt/galion
755 d root root /opt/galion/arena
755 d root root /opt/galion/arena/homes
755 d root root /opt/galion/arena/homes/g2-0
777 d root root /opt/galion/arena/homes/g2-0/sessions
700 d root root /opt/galion/hermes
777 d root root /opt/galion/hermes/vault
666 f root root /opt/galion/hermes/vault/.vault.lock
775 d root root /opt/galion/galion-light
775 d root root /opt/galion/galion-light/publish
775 d root root /opt/galion/galion-light/publish/obsidian
666 f root root /opt/galion/galion-light/publish/obsidian/link.env
755 d root root /opt/galion/console-dist
755 d root root /opt/galion/console-dist/static
666 f root root /opt/galion/console-dist/static/favicon.svg
644 f root root /opt/galion/console-dist/static/app.js
777 d root root /opt/galion/console-dist/node_modules/srvx
777 d root root /opt/galion/console-dist/node_modules/srvx/bin
777 f root root /opt/galion/console-dist/node_modules/srvx/bin/srvx.mjs
1777 d root root /opt/galion/tmp
666 f root root /opt/galion/tmp/scratch.txt
775 d tanga-proto tanga-proto /opt/galion/tanga
777 d tanga-proto tanga-proto /opt/galion/tanga/drop
";

#[test]
fn owner_fields_parse_and_old_format_still_works() {
    let e = parse_stat_line("777 d root root /opt/galion/arena/homes/g2-0/sessions").unwrap();
    assert_eq!(e.owner, Some(("root".into(), "root".into())));
    assert_eq!(e.path, "/opt/galion/arena/homes/g2-0/sessions");
    assert_eq!(e.kind, Kind::Dir);
    let e = parse_stat_line("644 f u g /a dir/with space").unwrap();
    assert_eq!(e.path, "/a dir/with space");
    assert_eq!(e.owner.as_ref().unwrap().0, "u");
    let e = parse_stat_line("644 f /a dir/with space").unwrap();
    assert_eq!(e.owner, None);
    assert_eq!(e.path, "/a dir/with space");
    let e = parse_stat_line("644 f 1000 1000 ./rel/x").unwrap();
    assert_eq!(e.owner, Some(("1000".into(), "1000".into())));
    assert_eq!(e.path, "./rel/x");
    // a relative path with no owner is left as is
    assert_eq!(
        parse_stat_line("644 f weird name").unwrap().path,
        "weird name"
    );
    // the findings are the same with or without owners
    let no_owner: String = VPS
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.splitn(5, ' ').collect();
            format!("{} {} {}\n", f[0], f[1], f[4])
        })
        .collect();
    assert_eq!(scan_stat_list(VPS).0, scan_stat_list(&no_owner).0);
}

#[test]
fn plan_from_vps_shape() {
    let entries = parse_stat_list(VPS);
    let plan = build_plan(&entries, &["/opt/galion/console-dist".to_string()]);
    let got: Vec<(&str, u32, u32, Reason)> = plan
        .rows
        .iter()
        .map(|r| (r.path.as_str(), r.old, r.new, r.reason))
        .collect();
    use Reason::*;
    assert_eq!(
        got,
        vec![
            (
                "/opt/galion/arena/homes/g2-0/sessions",
                0o777,
                0o775,
                DirOtherWritable
            ),
            (
                "/opt/galion/console-dist/node_modules/srvx",
                0o777,
                0o755,
                TreeNormalize
            ),
            (
                "/opt/galion/console-dist/node_modules/srvx/bin",
                0o777,
                0o755,
                TreeNormalize
            ),
            ("/opt/galion/tanga/drop", 0o777, 0o775, DirOtherWritable),
            (
                "/opt/galion/console-dist/node_modules/srvx/bin/srvx.mjs",
                0o777,
                0o755,
                TreeNormalize
            ),
            (
                "/opt/galion/console-dist/static/favicon.svg",
                0o666,
                0o644,
                TreeNormalize
            ),
            (
                "/opt/galion/galion-light/publish/obsidian/link.env",
                0o666,
                0o600,
                SecretWritable
            ),
        ],
        "dirs first; sticky /opt/galion/tmp and its 666 txt are left alone; 644 app.js untouched"
    );
    assert_eq!(plan.hidden, vec!["/opt/galion/hermes/vault".to_string()]);
    assert_eq!(
        plan.review,
        vec!["/opt/galion/tanga/drop (tanga-proto:tanga-proto)".to_string()]
    );
    // without --plan-tree the console-dist files are not planned, its dirs still are
    let p2 = build_plan(&entries, &[]);
    assert!(p2.rows.iter().all(|r| !r.path.ends_with("favicon.svg")));
    assert!(p2
        .rows
        .iter()
        .any(|r| r.path == "/opt/galion/console-dist/node_modules/srvx" && r.new == 0o775));
    // prefix match is per path component
    let p3 = build_plan(&entries, &["/opt/galion/console".to_string()]);
    assert!(p3.rows.iter().all(|r| r.reason != TreeNormalize));
}

#[test]
fn normalized_modes() {
    assert_eq!(normalized_mode(0o777, Kind::Dir), 0o755);
    assert_eq!(normalized_mode(0o700, Kind::Dir), 0o755);
    assert_eq!(normalized_mode(0o666, Kind::File), 0o644);
    assert_eq!(
        normalized_mode(0o4777, Kind::File),
        0o755,
        "set-uid dropped"
    );
    assert_eq!(normalized_mode(0o766, Kind::File), 0o755);
}

#[test]
fn tsv_round_trip_and_rollback() {
    let plan = build_plan(&parse_stat_list(VPS), &["/opt/galion/console-dist".into()]);
    let tsv = render_plan_tsv(&plan.rows, "test plan\nline two");
    assert!(tsv.starts_with("# test plan\n# line two\n# path\t"));
    let back = parse_plan_tsv(&tsv).unwrap();
    assert_eq!(back, plan.rows);
    let rb = rollback(&back);
    assert_eq!(rollback(&rb), back);
    assert!(rb.iter().all(|r| r.old != r.new));
    assert_eq!(rb[0].old, 0o775);
    assert_eq!(rb[0].new, 0o777);
    for (bad, why) in [
        ("/x\tf\troot:root\t666\t644", "6 tab"),
        ("x\tf\troot:root\t666\t644\ttree-normalize", "absolute"),
        ("/x\tq\troot:root\t666\t644\ttree-normalize", "type"),
        ("/x\tf\troot:root\t686\t644\ttree-normalize", "octal"),
        ("/x\tf\troot:root\t17777\t644\ttree-normalize", "octal"),
        ("/x\tf\troot:root\t666\t644\tnope", "reason"),
    ] {
        let e = parse_plan_tsv(bad).unwrap_err();
        assert!(e.contains(why) && e.contains("line 1"), "{bad:?} -> {e}");
    }
}

#[test]
fn verify_states() {
    let entries = parse_stat_list(VPS);
    let plan = build_plan(&entries, &["/opt/galion/console-dist".into()]);
    let v = verify_plan(&plan.rows, &entries);
    assert!(v.iter().all(|(_, s)| *s == RowState::Pending));
    let after = VPS
        .replace(
            "777 d root root /opt/galion/arena/homes/g2-0/sessions",
            "775 d root root /opt/galion/arena/homes/g2-0/sessions",
        )
        .replace(
            "666 f root root /opt/galion/console-dist/static/favicon.svg",
            "640 f root root /opt/galion/console-dist/static/favicon.svg",
        )
        .replace(
            "777 d tanga-proto tanga-proto /opt/galion/tanga/drop",
            "777 d root root /opt/galion/tanga/drop",
        )
        .replace(
            "666 f root root /opt/galion/galion-light/publish/obsidian/link.env\n",
            "",
        );
    let v = verify_plan(&plan.rows, &parse_stat_list(&after));
    let st = |p: &str| {
        v.iter()
            .find(|(r, _)| r.path.ends_with(p))
            .unwrap()
            .1
            .clone()
    };
    assert_eq!(st("g2-0/sessions"), RowState::Applied);
    assert_eq!(st("favicon.svg"), RowState::Drift(0o640));
    assert_eq!(st("tanga/drop"), RowState::OwnerChanged("root:root".into()));
    assert_eq!(st("link.env"), RowState::Missing);
    assert_eq!(st("srvx.mjs"), RowState::Pending);
    // owner '?' in the plan (list without owners) never reports OwnerChanged
    let mut rows = plan.rows.clone();
    for r in &mut rows {
        r.owner = "?".into();
    }
    let v = verify_plan(&rows, &parse_stat_list(&after));
    assert!(v
        .iter()
        .all(|(_, s)| !matches!(s, RowState::OwnerChanged(_))));
}

#[test]
fn sh_is_guarded_and_quoted() {
    let rows = vec![PlanRow {
        path: "/opt/it's here/d".into(),
        kind: Kind::Dir,
        owner: "root:root".into(),
        old: 0o777,
        new: 0o775,
        reason: Reason::DirOtherWritable,
    }];
    let sh = render_plan_sh(&rows, "t");
    assert!(
        sh.contains("g '/opt/it'\\''s here/d' d 777 'root:root' 775\n"),
        "{sh}"
    );
    assert!(sh.contains("STAGED, NOT APPLIED"));
    assert!(sh.contains("SKIP drift"));
    assert!(sh.contains("SPDX-License-Identifier: MIT"));
    assert_eq!(sh_quote("a'b"), "'a'\\''b'");
}
