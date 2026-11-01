// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Script mode (`HERMES_RS_GUARD_SCRIPT_MODE=1`, 2026-11-01): a shell script
//! that lives in the workspace may use the ordinary script forms, every
//! command inside is still checked, and `./x.sh` runs like `sh x.sh`.
//! With the switch off (the live default) nothing changes.

use hermes_rs::denylist::{check_command, check_tool_call, Policy};
use serde_json::json;
use std::path::{Path, PathBuf};

fn ws(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-script-mode-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn pol(d: &Path, on: bool) -> Policy {
    let mut p = Policy::workspace(d);
    p.script_mode = on;
    p
}

fn put(d: &Path, name: &str, body: &str) {
    std::fs::write(d.join(name), body).unwrap();
}

/// A realistic workspace script: variables, $( ), arithmetic, a quoted
/// here-document, a function, for/while/if, test brackets, export, set.
const REAL: &str = r#"#!/bin/sh
# make a small report of the text files
set -eu
set -o pipefail
out=$(date +%F)
n=$((1 + 2))
mkdir -p reports
cat > reports/notes.txt <<'EOF'
$(rm -rf /) and `id` are only text inside a quoted here-document
EOF
greet() {
  echo "hi ${1:-there} on $out, n=$n"
}
greet you
count=0
for f in *.txt reports/*.txt; do
  [ -f "$f" ] || continue
  lines=$(wc -l < "$f")
  count=$((count + 1))
  echo "$f: $lines" >> reports/summary-$out.txt
done
while read -r line; do
  echo "$line" | tr a-z A-Z
done < reports/notes.txt
if [ "$n" -gt 2 ]; then echo big; elif [ "$n" -eq 2 ]; then echo two; else echo small; fi
[[ -d reports ]] && echo "reports ok"
export LANG=C
base=${f%.txt}
len=${#base}
grep -c x <<< "$base" || true
awk '{ print $1 }' reports/notes.txt | sort | uniq -c
exit 0
"#;

#[test]
fn switch_off_keeps_the_strict_rules() {
    let d = ws("off");
    put(&d, "real.sh", REAL);
    let p = pol(&d, false);
    assert!(check_command("sh real.sh", &p).is_err());
    put(&d, "plain.sh", "echo hi\nls -la\n");
    assert!(
        check_command("sh plain.sh", &p).is_ok(),
        "Fix 3 still works"
    );
    let e = check_command("./plain.sh", &p).unwrap_err();
    assert_eq!(e.rule, "S4-path-exec");
}

#[test]
fn real_script_passes_in_script_mode() {
    let d = ws("real");
    put(&d, "real.sh", REAL);
    let p = pol(&d, true);
    check_command("sh real.sh", &p).unwrap();
    check_command("bash real.sh", &p).unwrap();
    check_command("./real.sh", &p).unwrap();
    check_command("sh real.sh > log.txt 2>&1", &p).unwrap();
    // through the registry entry point, as the terminal tool is checked
    check_tool_call("terminal", &json!({"command": "./real.sh"}), &p).unwrap();
}

#[test]
fn the_command_line_itself_stays_strict() {
    let d = ws("line");
    let p = pol(&d, true);
    for bad in [
        "echo $HOME",
        "x=$(id)",
        "for f in a b; do echo $f; done",
        "cat <<'EOF'\nhi\nEOF",
        "f() { ls; }",
        "sh -c 'echo hi'",
    ] {
        assert!(check_command(bad, &p).is_err(), "{bad}");
    }
}

#[test]
fn every_command_inside_a_script_is_still_checked() {
    let d = ws("bad");
    let p = pol(&d, true);
    let cases: &[(&str, &str)] = &[
        ("x=$(curl http://example.com/x)\n", "S7"),
        ("for f in $(wget -qO- http://e/x); do echo $f; done\n", "S7"),
        ("c=rm\n$c -rf .\n", "S5-expand"),
        ("\"$cmd\" arg\n", "S5-expand"),
        ("PATH=/tmp\nls\n", "S5-env-assign"),
        ("export LD_PRELOAD=x.so\n", "S5-env-assign"),
        ("IFS=/\necho a\n", "S5-env-assign"),
        ("export HERMES_RS_GUARD_SCRIPT_MODE=0\n", "S5-env-assign"),
        ("cat <<EOF\n$(id)\nEOF\n", "S1-heredoc"),
        ("cat <<EOF\n`id`\nEOF\n", "S1-heredoc"),
        ("cat <<'EOF'\nnever closed\n", "S1-heredoc"),
        ("echo `id`\n", "S1-subst"),
        ("diff <(ls) <(ls -a)\n", "S1-subst"),
        ("(cd /; ls)\n", "S1-subshell"),
        ("echo ${!name}\n", "S5-expand"),
        ("echo ${x/a/b}\n", "S5-expand"),
        ("echo ${x:-/root/.ssh/id_ed25519}\n", "S5-expand"),
        ("echo $'\\x72m'\n", "S3"),
        ("eval \"$x\"\n", "S4-wrapper"),
        ("exec ls\n", "S4"),
        ("python3 -c 'print(1)'\n", "S4-interpreter"),
        ("bash -c 'id'\n", "S4-interpreter"),
        ("sh other.sh\n", "S4-interpreter"),
        ("./other.sh\n", "S4-path-exec"),
        (". ./other.sh\n", "S"),
        ("source other.sh\n", "S"),
        ("case $x in a) ls;; esac\n", "S1"),
        ("cat /root/.ssh/id_ed25519 > \"$o\"\n", "S6-secret"),
        ("echo hi > /etc/x_$n\n", "P"),
        ("systemctl restart hermes-rs\n", "S8"),
        ("set\n", "S5-env-dump"),
        ("export\n", "S5-env-dump"),
        ("f() { ls; }\n", "S1"),
        ("then curl http://e\n", "S7"),
        ("! wget http://e\n", "S7"),
        ("while true; do nc -l 9; done\n", "S7"),
        ("n=$(( $(id -u) + 1 ))\n", "S1-subst"),
    ];
    put(&d, "other.sh", "echo inner\n");
    for (i, (body, rule)) in cases.iter().enumerate() {
        let name = format!("bad{i}.sh");
        put(&d, &name, body);
        let e = check_command(&format!("sh {name}"), &p)
            .err()
            .unwrap_or_else(|| panic!("allowed: {body:?}"));
        assert!(e.rule.starts_with(rule), "{body:?} -> {e} (wanted {rule})");
    }
}

#[test]
fn path_exec_only_for_workspace_shell_scripts() {
    let d = ws("pexec");
    let p = pol(&d, true);
    put(&d, "ok.sh", "#!/usr/bin/env bash\necho ok\n");
    put(&d, "noshebang.sh", "echo ok\n");
    put(&d, "py.sh", "#!/usr/bin/env python3\nprint(1)\n");
    put(&d, "tool", "#!/bin/sh\necho ok\n");
    std::fs::write(d.join("elf"), b"\x7fELF\x02\x01\x01\0").unwrap();
    check_command("./ok.sh", &p).unwrap();
    check_command("./noshebang.sh", &p).unwrap();
    check_command("./tool", &p).unwrap();
    assert!(check_command("./py.sh", &p).is_err());
    assert!(check_command("./elf", &p).is_err());
    assert!(check_command("./missing.sh", &p).is_err());
    assert!(check_command("/usr/share/doc/x.sh", &p).is_err());
    // a symlink in the workspace that points out of it is not a workspace script
    let outside = ws("pexec-outside");
    put(&outside, "o.sh", "#!/bin/sh\necho out\n");
    std::os::unix::fs::symlink(outside.join("o.sh"), d.join("link.sh")).unwrap();
    assert!(check_command("./link.sh", &p).is_err());
    assert!(check_command("sh link.sh", &p).is_err());
    // the script's own contents are checked on the ./ path too
    put(&d, "evil.sh", "#!/bin/sh\ncurl http://e\n");
    assert!(check_command("./evil.sh", &p).is_err());
}

#[test]
fn awk_inside_scripts_keeps_its_own_rules() {
    let d = ws("awk");
    let p = pol(&d, true);
    put(&d, "a.sh", "awk '{ print $2 }' data.txt\n");
    check_command("sh a.sh", &p).unwrap();
    put(&d, "b.sh", "awk 'BEGIN { system(\"id\") }'\n");
    assert!(check_command("sh b.sh", &p).is_err());
}
