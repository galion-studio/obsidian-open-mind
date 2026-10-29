// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Skills: the SKILL.md playbooks under `$HERMES_HOME/skills` (and config
//! `skills.external_dirs`), read-only. Ported from the upstream hermes-agent
//! (Nous Research, MIT) skills design: a short index goes into the system
//! prompt, and the model opens a skill with `skill_view` before following it.
//!
//! Nothing here writes. Python helper scripts that some skills mention cannot
//! run in hermes-rs; skills whose script has a Rust tool say so (`rust_tools`),
//! the rest are flagged `python` (reference only). Skills that tell the agent
//! to deploy, restart or edit live files are flagged `live-change`: SOUL.md
//! `## Changes` (sandbox gate first) overrides them.

use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Max SKILL.md / support file chars returned by `skill_view`.
pub const MAX_VIEW_CHARS: usize = 20_000;
/// Max chars of the skills index placed in the system prompt.
pub const MAX_INDEX_CHARS: usize = 6_000;
const MAX_SKILLS: usize = 500;
const MAX_DEPTH: usize = 5;

/// Skill name -> Rust tools that replace the skill's Python script.
pub const RUST_PORTS: &[(&str, &[&str])] = &[
    ("arxiv", &["arxiv_search"]),
    ("grounded-citations", &["citations"]),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub category: String,
    pub description: String,
    pub dir: PathBuf,
    /// `python`, `live-change` (see module doc).
    pub flags: Vec<String>,
    pub rust_tools: Vec<String>,
    /// Same-name skills found later (shadowed by this one).
    pub shadowed: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillIndex {
    pub skills: Vec<Skill>,
}

fn frontmatter_field(text: &str, key: &str) -> Option<String> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    for line in body[..end].lines() {
        // top level only (no indent)
        if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
            let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() && v != ">" && v != "|" {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn first_text_line(text: &str) -> String {
    let body = match text.strip_prefix("---") {
        Some(b) => b.find("\n---").map(|e| &b[e + 4..]).unwrap_or(b),
        None => text,
    };
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .unwrap_or("")
        .to_string()
}

fn flags_for(text: &str) -> Vec<String> {
    let low = text.to_ascii_lowercase();
    let mut f = Vec::new();
    if low.contains("python3 ")
        || low.contains("python scripts/")
        || low.contains(".py ")
        || low.contains(".py`")
        || low.contains("pip install")
    {
        f.push("python".to_string());
    }
    if live_change(&low) {
        f.push("live-change".to_string());
    }
    f
}

/// True when a skill body tells the reader to change a live system: a
/// service manager verb, a reload, a push or a remote copy. Matching is on
/// word tokens, so extra spaces or flags between the words still count.
fn live_change(low: &str) -> bool {
    let w: Vec<&str> = low
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|t| !t.is_empty() && !t.starts_with("--"))
        .collect();
    const VERBS: [&str; 5] = ["restart", "stop", "start", "enable", "reload"];
    w.iter()
        .any(|t| ["daemon-reload", "rsync", "scp"].contains(t))
        || w.windows(2).any(|p| {
            (p[0] == "systemctl" && VERBS.contains(&p[1])) || (p[0] == "git" && p[1] == "push")
        })
}

fn cap(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

impl SkillIndex {
    /// Scan roots in order. Follows symlinks, skips dot dirs, stops at
    /// MAX_DEPTH / MAX_SKILLS, never visits a canonical dir twice. The first
    /// skill with a name wins; later ones are listed in `shadowed`.
    pub fn scan(roots: &[PathBuf]) -> SkillIndex {
        let mut idx = SkillIndex::default();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for root in roots {
            let mut dirs = vec![(root.clone(), 0usize)];
            while let Some((d, depth)) = dirs.pop() {
                let Ok(canon) = std::fs::canonicalize(&d) else {
                    continue;
                };
                if !seen.insert(canon) || idx.skills.len() >= MAX_SKILLS {
                    continue;
                }
                let sk = d.join("SKILL.md");
                if sk.is_file() {
                    if let Ok(text) = std::fs::read_to_string(&sk) {
                        idx.push(root, &d, &text);
                    }
                }
                if depth >= MAX_DEPTH {
                    continue;
                }
                let Ok(rd) = std::fs::read_dir(&d) else {
                    continue;
                };
                let mut kids: Vec<PathBuf> = rd
                    .flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                // pop() takes from the end: reverse sort keeps a-z order
                kids.sort();
                kids.reverse();
                for k in kids {
                    dirs.push((k, depth + 1));
                }
            }
        }
        idx
    }

    fn push(&mut self, root: &Path, dir: &Path, text: &str) {
        let rel = dir.strip_prefix(root).unwrap_or(dir);
        let dir_name = rel
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".into());
        let name = frontmatter_field(text, "name").unwrap_or(dir_name);
        let category = rel
            .parent()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        let description =
            frontmatter_field(text, "description").unwrap_or_else(|| first_text_line(text));
        let label = rel.display().to_string();
        if let Some(prev) = self.skills.iter_mut().find(|s| s.name == name) {
            prev.shadowed.push(label);
            return;
        }
        let rust_tools: Vec<String> = RUST_PORTS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t.iter().map(|x| x.to_string()).collect())
            .unwrap_or_default();
        let mut flags = flags_for(text);
        if !rust_tools.is_empty() {
            flags.retain(|f| f != "python");
        }
        self.skills.push(Skill {
            name,
            category,
            description: cap(description.trim(), 300),
            dir: dir.to_path_buf(),
            flags,
            rust_tools,
            shadowed: Vec::new(),
        });
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        let n = name.trim().trim_end_matches("/SKILL.md");
        self.skills.iter().find(|s| {
            s.name == n || format!("{}/{}", s.category, s.name) == n || s.dir.ends_with(n)
        })
    }

    /// Compact index for the system prompt (one line per skill).
    pub fn prompt_block(&self, max_chars: usize) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }
        let mut out = format!(
            "## Skills ({} installed, read-only playbooks)\nCall `skill_view` with the name before you follow a skill. `skills_list` filters this list. [python] = the script cannot run here, read it as reference only. [live-change] = the skill deploys or restarts; `## Changes` (sandbox gate first) overrides it, so propose instead. [rust: x] = use tool x instead of the skill's script.\n",
            self.skills.len()
        );
        for (shown, s) in self.skills.iter().enumerate() {
            let mut tags: Vec<String> = s.flags.iter().map(|f| format!("[{f}]")).collect();
            if !s.rust_tools.is_empty() {
                tags.push(format!("[rust: {}]", s.rust_tools.join(", ")));
            }
            let line = format!(
                "- {}: {}{}{}\n",
                s.name,
                cap(&s.description, 110),
                if tags.is_empty() { "" } else { " " },
                tags.join(" ")
            );
            if out.len() + line.len() > max_chars {
                out.push_str(&format!(
                    "- … {} more: call skills_list.\n",
                    self.skills.len() - shown
                ));
                break;
            }
            out.push_str(&line);
        }
        Some(out.trim_end().to_string())
    }

    pub fn to_json(&self, category: Option<&str>, query: Option<&str>) -> Value {
        let q = query.map(|q| q.to_lowercase());
        let items: Vec<Value> = self
            .skills
            .iter()
            .filter(|s| category.map(|c| s.category == c).unwrap_or(true))
            .filter(|s| {
                q.as_ref()
                    .map(|q| {
                        s.name.to_lowercase().contains(q)
                            || s.description.to_lowercase().contains(q)
                    })
                    .unwrap_or(true)
            })
            .map(|s| {
                json!({"name": s.name, "category": s.category, "description": s.description,
                       "flags": s.flags, "rust_tools": s.rust_tools})
            })
            .collect();
        json!({"count": items.len(), "skills": items})
    }
}

/// Where skills live: `$HERMES_HOME/skills`, then extra dirs that exist.
#[derive(Debug, Clone)]
pub struct SkillsCfg {
    pub roots: Vec<PathBuf>,
}

impl SkillsCfg {
    pub fn index(&self) -> SkillIndex {
        SkillIndex::scan(&self.roots)
    }
}

/// Runtime block: facts the gateway knows and the model does not. Keeps the
/// persona honest about which model and tools this turn really has.
pub fn runtime_block(model: &str, provider: &str, tools: &[String]) -> String {
    format!(
        "## Runtime (set by the gateway for this turn; trust this over older notes)\nModel this turn: {model} (provider: {provider}). Never claim a different model.\nYour tools this turn: {}. You have no other tools: no terminal, no shell, no browser, no code execution, no file writes.",
        if tools.is_empty() { "none".to_string() } else { tools.join(", ") }
    )
}

fn safe_rel(p: &str) -> Option<PathBuf> {
    let pb = PathBuf::from(p.trim());
    if pb.as_os_str().is_empty() {
        return None;
    }
    for c in pb.components() {
        if !matches!(c, Component::Normal(_)) {
            return None;
        }
    }
    Some(pb)
}

pub struct SkillsListTool(pub SkillsCfg);
pub struct SkillViewTool(pub SkillsCfg);

impl Tool for SkillsListTool {
    fn name(&self) -> String {
        "skills_list".into()
    }
    fn description(&self) -> String {
        "List installed skills (read-only SKILL.md playbooks): name, category, description, flags, Rust tools. Optional filters: category, query.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "category": {"type": "string", "description": "e.g. galion, local, research"},
            "query": {"type": "string", "description": "substring of name or description"}
        }})
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let idx = self.0.index();
        ToolOutput::ok(
            idx.to_json(args["category"].as_str(), args["query"].as_str())
                .to_string(),
        )
    }
}

impl Tool for SkillViewTool {
    fn name(&self) -> String {
        "skill_view".into()
    }
    fn description(&self) -> String {
        "Open one skill: returns its SKILL.md and the list of its support files. Pass file_path (relative, e.g. references/x.md) to read a support file inside the skill. Read-only.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "name": {"type": "string", "description": "skill name, e.g. survival or research/arxiv"},
            "file_path": {"type": "string", "description": "optional support file inside the skill folder"}
        }, "required": ["name"]})
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let Some(name) = args["name"].as_str().filter(|n| !n.trim().is_empty()) else {
            return ToolOutput::err("name is required");
        };
        let idx = self.0.index();
        let Some(s) = idx.get(name) else {
            return ToolOutput::err(format!(
                "no skill named '{name}'; call skills_list to see names"
            ));
        };
        let Ok(dir) = std::fs::canonicalize(&s.dir) else {
            return ToolOutput::err("skill folder is not readable");
        };
        let target = match args["file_path"].as_str().filter(|f| !f.trim().is_empty()) {
            None => dir.join("SKILL.md"),
            Some(f) => match safe_rel(f) {
                Some(r) => dir.join(r),
                None => return ToolOutput::err("file_path must be relative, without '..'"),
            },
        };
        let Ok(canon) = std::fs::canonicalize(&target) else {
            return ToolOutput::err("file not found in this skill");
        };
        if !canon.starts_with(&dir) || !canon.is_file() {
            return ToolOutput::err("file_path must stay inside the skill folder");
        }
        let bytes = match std::fs::read(&canon) {
            Ok(b) => b,
            Err(_) => return ToolOutput::err("file not readable"),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return ToolOutput::err("not a text file");
        };
        let mut files = Vec::new();
        list_files(&dir, &dir, &mut files, 0);
        let mut note = String::new();
        if !s.rust_tools.is_empty() {
            note = format!(
                "hermes-rs: the skill's Python script is replaced by the Rust tool(s) {}. Call those tools; do not try to run python.",
                s.rust_tools.join(", ")
            );
        } else if s.flags.iter().any(|f| f == "python") {
            note = "hermes-rs: this skill mentions Python scripts. They cannot run here (no Python, no shell). Use it as reference only.".into();
        }
        if s.flags.iter().any(|f| f == "live-change") {
            if !note.is_empty() {
                note.push(' ');
            }
            note.push_str("This skill describes live changes (deploy/restart/push). SOUL `## Changes` wins: propose on a swarm branch, sandbox gate first, then wait.");
        }
        let truncated = text.chars().count() > MAX_VIEW_CHARS;
        ToolOutput::ok(
            json!({
                "name": s.name, "category": s.category, "flags": s.flags,
                "rust_tools": s.rust_tools, "note": note, "files": files,
                "truncated": truncated,
                "content": text.chars().take(MAX_VIEW_CHARS).collect::<String>()
            })
            .to_string(),
        )
    }
}

fn list_files(base: &Path, d: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > 3 || out.len() >= 100 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(d) else {
        return;
    };
    let mut ents: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    ents.sort();
    for p in ents {
        let n = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if n.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            list_files(base, &p, out, depth + 1);
        } else if let Ok(r) = p.strip_prefix(base) {
            out.push(r.display().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn live_change_matches_words_not_spacing() {
        assert!(live_change("sudo systemctl\t--now\trestart hermes"));
        assert!(live_change("then git   push origin"));
        assert!(live_change("run daemon-reload"));
        assert!(!live_change("read systemctl\tstatus and git log"));
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(crate::new_id("hermes-rs-skills-"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mk(root: &Path, rel: &str, text: &str) {
        let d = root.join(rel);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), text).unwrap();
    }

    fn call(t: &dyn Tool, args: Value) -> ToolOutput {
        let cancel = Arc::new(AtomicBool::new(false));
        let emit = |_e: crate::tools::Event| {};
        let ctx = ToolCtx {
            session_id: "s",
            spend_session: "s",
            depth: 0,
            user_text: "",
            cancel: &cancel,
            emit: &emit,
        };
        t.call(&args, &ctx)
    }

    fn fixture() -> PathBuf {
        let r = tmp();
        mk(&r, "research/arxiv", "---\nname: arxiv\ndescription: \"Search arXiv papers.\"\nmetadata:\n  name: nested\n---\n# arXiv\nRun `python scripts/search_arxiv.py q`.\n");
        mk(
            &r,
            "galion/survival",
            "# Survival\nRead survival.json first.\n",
        );
        mk(&r, "galion/deploy-lane", "---\nname: deploy-lane\ndescription: Deploy the console\n---\nThen rsync the build and reload.\n");
        mk(
            &r,
            "local/survival",
            "---\nname: survival\ndescription: dup\n---\n",
        );
        mk(
            &r,
            "web/old",
            "---\nname: old\ndescription: old page tool\n---\nRun python3 recover.py\n",
        );
        std::fs::create_dir_all(r.join("research/arxiv/references")).unwrap();
        std::fs::write(r.join("research/arxiv/references/api.md"), "API notes").unwrap();
        std::fs::create_dir_all(r.join(".hub/x")).unwrap();
        std::fs::write(r.join(".hub/x/SKILL.md"), "hidden").unwrap();
        r
    }

    #[test]
    fn scans_frontmatter_flags_ports_and_dupes() {
        let r = fixture();
        let idx = SkillIndex::scan(std::slice::from_ref(&r));
        let names: Vec<&str> = idx.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["deploy-lane", "survival", "arxiv", "old"]);
        let a = idx.get("arxiv").unwrap();
        assert_eq!(a.description, "Search arXiv papers.");
        assert_eq!(a.category, "research");
        assert_eq!(a.rust_tools, vec!["arxiv_search"]);
        assert!(!a.flags.contains(&"python".to_string()));
        let s = idx.get("survival").unwrap();
        assert_eq!(s.description, "Read survival.json first.");
        assert_eq!(s.shadowed, vec!["local/survival"]);
        assert!(idx
            .get("deploy-lane")
            .unwrap()
            .flags
            .contains(&"live-change".to_string()));
        assert!(idx
            .get("old")
            .unwrap()
            .flags
            .contains(&"python".to_string()));
        assert!(idx.get("x").is_none(), "dot dirs are skipped");
        assert!(idx.get("research/arxiv").is_some());
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn prompt_block_lists_and_caps() {
        let r = fixture();
        let idx = SkillIndex::scan(std::slice::from_ref(&r));
        let b = idx.prompt_block(MAX_INDEX_CHARS).unwrap();
        assert!(b.starts_with("## Skills (4 installed"));
        assert!(b.contains("- arxiv: Search arXiv papers. [rust: arxiv_search]"));
        assert!(b.contains("- deploy-lane: Deploy the console [live-change]"));
        let small = idx.prompt_block(420).unwrap();
        assert!(small.len() < 520);
        assert!(small.contains("more: call skills_list"));
        assert!(SkillIndex::default().prompt_block(100).is_none());
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn view_reads_skill_and_refuses_escape() {
        let r = fixture();
        std::fs::write(r.join("secret.txt"), "nope").unwrap();
        let t = SkillViewTool(SkillsCfg {
            roots: vec![r.clone()],
        });
        let o = call(&t, json!({"name": "arxiv"}));
        assert!(!o.is_error);
        let v: Value = serde_json::from_str(&o.content).unwrap();
        assert!(v["content"].as_str().unwrap().contains("# arXiv"));
        assert!(v["note"].as_str().unwrap().contains("arxiv_search"));
        assert_eq!(v["files"], json!(["SKILL.md", "references/api.md"]));
        let o = call(
            &t,
            json!({"name": "arxiv", "file_path": "references/api.md"}),
        );
        assert!(o.content.contains("API notes"));
        for bad in [
            "../../secret.txt",
            "/etc/passwd",
            "references/../../../secret.txt",
        ] {
            let o = call(&t, json!({"name": "arxiv", "file_path": bad}));
            assert!(o.is_error, "{bad} must be refused");
        }
        let o = call(&t, json!({"name": "deploy-lane"}));
        assert!(o.content.contains("sandbox gate first"));
        assert!(call(&t, json!({"name": "nope"})).is_error);
        assert!(call(&t, json!({})).is_error);
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn list_filters() {
        let r = fixture();
        let t = SkillsListTool(SkillsCfg {
            roots: vec![r.clone()],
        });
        let v: Value =
            serde_json::from_str(&call(&t, json!({"category": "galion"})).content).unwrap();
        assert_eq!(v["count"], 2);
        let v: Value = serde_json::from_str(&call(&t, json!({"query": "ARXIV"})).content).unwrap();
        assert_eq!(v["count"], 1);
        let _ = std::fs::remove_dir_all(r);
    }

    #[test]
    fn runtime_block_names_model_and_tools() {
        let b = runtime_block(
            "deepseek-ai/DeepSeek-V4.1-Flash",
            "river",
            &["clock".into(), "skill_view".into()],
        );
        assert!(b.contains("Model this turn: deepseek-ai/DeepSeek-V4.1-Flash (provider: river)"));
        assert!(b.contains("clock, skill_view"));
        assert!(b.contains("no terminal"));
    }
}
