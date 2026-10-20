// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Results model plus the two report writers: JUnit XML (for CI) and a
//! Markdown report. Every timestamp is Europe/Zagreb with an explicit offset.

use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    /// Failed, but listed in the known-failures file: reported, not fatal.
    KnownFail,
    /// Listed as a known failure but passed: reported so the list gets pruned.
    Fixed,
    Skip,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::Fail => "FAIL",
            Outcome::KnownFail => "KNOWN-FAIL",
            Outcome::Fixed => "PASS (was known-fail)",
            Outcome::Skip => "SKIP",
        }
    }
    pub fn failed(self) -> bool {
        matches!(self, Outcome::Fail | Outcome::KnownFail)
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub outcome: Outcome,
    pub detail: String,
    pub ms: u128,
}

#[derive(Debug, Clone)]
pub struct Suite {
    /// Scenario id, e.g. `one-pairing`.
    pub name: String,
    /// Service under test, e.g. `galion-one`.
    pub service: String,
    pub started_at: String,
    pub checks: Vec<Check>,
    pub log_tail: String,
}

impl Suite {
    pub fn new(name: &str, service: &str) -> Suite {
        Suite {
            name: name.into(),
            service: service.into(),
            started_at: zagreb_time::now_rfc3339(),
            checks: Vec::new(),
            log_tail: String::new(),
        }
    }

    /// Run one check, time it and record it.
    pub fn check(&mut self, name: &str, f: impl FnOnce() -> Result<String, String>) -> bool {
        let t = Instant::now();
        let r = f();
        let ms = t.elapsed().as_millis();
        let (outcome, detail) = match r {
            Ok(d) => (Outcome::Pass, d),
            Err(d) => (Outcome::Fail, d),
        };
        self.checks.push(Check {
            name: name.into(),
            outcome,
            detail,
            ms,
        });
        outcome == Outcome::Pass
    }

    pub fn skip(&mut self, name: &str, why: &str) {
        self.checks.push(Check {
            name: name.into(),
            outcome: Outcome::Skip,
            detail: why.into(),
            ms: 0,
        });
    }

    pub fn ms(&self) -> u128 {
        self.checks.iter().map(|c| c.ms).sum()
    }

    pub fn count(&self, f: impl Fn(Outcome) -> bool) -> usize {
        self.checks.iter().filter(|c| f(c.outcome)).count()
    }

    /// Worst outcome, for the per-service summary.
    pub fn verdict(&self) -> Outcome {
        let has = |o: Outcome| self.checks.iter().any(|c| c.outcome == o);
        if has(Outcome::Fail) {
            Outcome::Fail
        } else if has(Outcome::KnownFail) {
            Outcome::KnownFail
        } else if self.checks.iter().all(|c| c.outcome == Outcome::Skip) {
            Outcome::Skip
        } else {
            Outcome::Pass
        }
    }
}

/// `scenario/check` ids listed in a known-failures file (one per line, `#`
/// comments). A failing listed check becomes KNOWN-FAIL; a passing one Fixed.
pub fn apply_known(suites: &mut [Suite], known: &[String]) {
    for s in suites.iter_mut() {
        for c in s.checks.iter_mut() {
            let id = format!("{}/{}", s.name, c.name);
            if !known.iter().any(|k| k == &id) {
                continue;
            }
            c.outcome = match c.outcome {
                Outcome::Fail => Outcome::KnownFail,
                Outcome::Pass => Outcome::Fixed,
                o => o,
            };
        }
    }
}

pub fn parse_known(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn xml_attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\n' => o.push_str("&#10;"),
            '\t' | '\r' => o.push(' '),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

fn cdata(s: &str) -> String {
    let clean: String = s
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || (*c as u32) >= 0x20)
        .collect();
    format!("<![CDATA[{}]]>", clean.replace("]]>", "]]]]><![CDATA[>"))
}

fn secs(ms: u128) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// JUnit XML. KNOWN-FAIL is written as `<skipped>` so CI stays readable;
/// the Markdown report keeps the full truth.
pub fn junit(title: &str, suites: &[Suite]) -> String {
    let all = |f: &dyn Fn(Outcome) -> bool| suites.iter().map(|s| s.count(f)).sum::<usize>();
    let tests = all(&|_| true);
    let failures = all(&|o| o == Outcome::Fail);
    let skipped = all(&|o| matches!(o, Outcome::Skip | Outcome::KnownFail));
    let total_ms: u128 = suites.iter().map(Suite::ms).sum();
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!(
        "<testsuites name=\"{}\" tests=\"{tests}\" failures=\"{failures}\" errors=\"0\" skipped=\"{skipped}\" time=\"{}\" timestamp=\"{}\">\n",
        xml_attr(title),
        secs(total_ms),
        xml_attr(&zagreb_time::now_rfc3339())
    ));
    for s in suites {
        x.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{}\" timestamp=\"{}\">\n",
            xml_attr(&s.name),
            s.checks.len(),
            s.count(|o| o == Outcome::Fail),
            s.count(|o| matches!(o, Outcome::Skip | Outcome::KnownFail)),
            secs(s.ms()),
            xml_attr(&s.started_at)
        ));
        for c in &s.checks {
            x.push_str(&format!(
                "    <testcase classname=\"realtest.{}.{}\" name=\"{}\" time=\"{}\"",
                xml_attr(&s.service),
                xml_attr(&s.name),
                xml_attr(&c.name),
                secs(c.ms)
            ));
            match c.outcome {
                Outcome::Pass | Outcome::Fixed => {
                    x.push_str(">\n");
                    x.push_str(&format!(
                        "      <system-out>{}</system-out>\n",
                        cdata(&c.detail)
                    ));
                }
                Outcome::Fail => {
                    x.push_str(">\n");
                    x.push_str(&format!(
                        "      <failure message=\"{}\">{}</failure>\n",
                        xml_attr(&first_line(&c.detail)),
                        cdata(&c.detail)
                    ));
                }
                Outcome::KnownFail => {
                    x.push_str(">\n");
                    x.push_str(&format!(
                        "      <skipped message=\"{}\"/>\n",
                        xml_attr(&format!("KNOWN-FAIL: {}", first_line(&c.detail)))
                    ));
                }
                Outcome::Skip => {
                    x.push_str(">\n");
                    x.push_str(&format!(
                        "      <skipped message=\"{}\"/>\n",
                        xml_attr(&first_line(&c.detail))
                    ));
                }
            }
            x.push_str("    </testcase>\n");
        }
        if !s.log_tail.is_empty() {
            x.push_str(&format!(
                "    <system-out>{}</system-out>\n",
                cdata(&s.log_tail)
            ));
        }
        x.push_str("  </testsuite>\n");
    }
    x.push_str("</testsuites>\n");
    x
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(200).collect()
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
        .replace('\n', " / ")
        .chars()
        .take(300)
        .collect()
}

/// Markdown report. `preamble` is free text (commands, environment, timings).
pub fn markdown(title: &str, preamble: &str, suites: &[Suite]) -> String {
    let mut m = format!(
        "# {title}\n\nWritten {} (Europe/Zagreb).\n\n",
        zagreb_time::now_rfc3339()
    );
    m.push_str(preamble);
    if !preamble.ends_with('\n') {
        m.push('\n');
    }
    let n = |f: &dyn Fn(Outcome) -> bool| suites.iter().map(|s| s.count(f)).sum::<usize>();
    m.push_str(&format!(
        "\n## Summary\n\nChecks: {} total, {} pass, {} FAIL, {} known-fail, {} skipped.\n\n",
        n(&|_| true),
        n(&|o| matches!(o, Outcome::Pass | Outcome::Fixed)),
        n(&|o| o == Outcome::Fail),
        n(&|o| o == Outcome::KnownFail),
        n(&|o| o == Outcome::Skip)
    ));
    m.push_str("| Scenario | Service | Verdict | Pass | Fail | Time (ms) | Started |\n|---|---|---|---|---|---|---|\n");
    for s in suites {
        m.push_str(&format!(
            "| {} | {} | **{}** | {} | {} | {} | {} |\n",
            s.name,
            s.service,
            s.verdict().label(),
            s.count(|o| matches!(o, Outcome::Pass | Outcome::Fixed)),
            s.count(Outcome::failed),
            s.ms(),
            s.started_at
        ));
    }
    m.push_str("\n## Checks\n");
    for s in suites {
        m.push_str(&format!(
            "\n### {} ({})\n\n| Check | Result | ms | Detail |\n|---|---|---|---|\n",
            s.name, s.service
        ));
        for c in &s.checks {
            m.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                c.name,
                c.outcome.label(),
                c.ms,
                md_cell(&c.detail)
            ));
        }
        if !s.log_tail.is_empty() {
            m.push_str(&format!(
                "\n<details><summary>log tail</summary>\n\n```\n{}\n```\n</details>\n",
                s.log_tail.trim_end()
            ));
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Suite> {
        let mut s = Suite::new("one-pairing", "galion-one");
        s.check("ok", || Ok("fine <b>".into()));
        s.check("bad", || Err("boom & \"quote\"\nline2 ]]> end".into()));
        s.skip("later", "no binary");
        vec![s]
    }

    #[test]
    fn junit_counts_and_escapes() {
        let x = junit("t", &sample());
        assert!(x.contains("tests=\"3\" failures=\"1\""));
        assert!(x.contains("boom &amp; &quot;quote&quot;"));
        assert!(x.contains("]]]]><![CDATA[>"));
        assert!(x.contains("+02:00") || x.contains("+01:00"));
    }

    #[test]
    fn known_failures_downgrade_and_flag_fixed() {
        let mut s = sample();
        s[0].check("was-broken", || Ok(String::new()));
        let known = parse_known("# c\none-pairing/bad  # fail-open\none-pairing/was-broken\n");
        apply_known(&mut s, &known);
        assert_eq!(s[0].checks[1].outcome, Outcome::KnownFail);
        assert_eq!(s[0].checks[3].outcome, Outcome::Fixed);
        assert_eq!(s[0].verdict(), Outcome::KnownFail);
        let md = markdown("r", "pre", &s);
        assert!(md.contains("KNOWN-FAIL") && md.contains("1 known-fail"));
    }
}
