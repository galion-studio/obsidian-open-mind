// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tool registry and the tool trait the agent loop calls.

use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Events a turn emits while it runs (streamed to SSE clients).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Delta(String),
    Reasoning(String),
    ToolStarted {
        tool: String,
        preview: String,
    },
    ToolCompleted {
        tool: String,
        duration: f64,
        error: bool,
        preview: String,
    },
    Status(String),
}

pub struct ToolCtx<'a> {
    pub session_id: &'a str,
    /// Session the spend is charged to (the parent for subagents).
    pub spend_session: &'a str,
    pub depth: usize,
    pub user_text: &'a str,
    pub cancel: &'a Arc<AtomicBool>,
    pub emit: &'a (dyn Fn(Event) + Sync),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(s: impl Into<String>) -> Self {
        ToolOutput {
            content: s.into(),
            is_error: false,
        }
    }
    /// Upstream `tool_error`: a JSON `{"error": ...}` body.
    pub fn err(s: impl AsRef<str>) -> Self {
        ToolOutput {
            content: json!({"error": s.as_ref()}).to_string(),
            is_error: true,
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    /// JSON Schema of the parameters.
    fn parameters(&self) -> Value;
    fn description(&self) -> String;
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput;
    /// Name the deny list classifies this tool by (shell / write / code
    /// hints). Defaults to `name()`; MCP tools add their server's class.
    fn deny_name(&self) -> String {
        self.name()
    }
}

#[derive(Clone, Default)]
pub struct Registry {
    tools: Vec<Arc<dyn Tool>>,
    /// Deny-list policy for tools added from now on (None = process-wide).
    policy: Option<Arc<crate::denylist::Policy>>,
}

/// Every registered tool is wrapped in this: the deny list runs before the
/// tool, for the boss agent, delegated children, cron and MCP tools alike.
/// There is no switch that turns it off.
pub struct Guarded {
    inner: Arc<dyn Tool>,
    policy: Arc<crate::denylist::Policy>,
}

impl Tool for Guarded {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn parameters(&self) -> Value {
        self.inner.parameters()
    }
    fn description(&self) -> String {
        self.inner.description()
    }
    fn deny_name(&self) -> String {
        self.inner.deny_name()
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let name = self.inner.name();
        let deny_name = self.inner.deny_name();
        match crate::denylist::check_tool_call(&deny_name, args, &self.policy) {
            Ok(()) => {
                // full-write mode: keep an undo copy of what the call may
                // delete or overwrite; no copy = the call does not run
                let plan = crate::denylist::undo::plan(&deny_name, args, &self.policy);
                let saved = match crate::denylist::undo::snapshot(
                    &plan,
                    &self.policy,
                    &name,
                    &args.to_string(),
                ) {
                    Ok(s) => s,
                    Err(d) => {
                        eprintln!(
                            "hermes-rs denylist: refused tool={name} rule={} session={} depth={}",
                            d.rule, ctx.session_id, ctx.depth
                        );
                        return ToolOutput::err(crate::denylist::refusal_text(&d));
                    }
                };
                let mut out = self.inner.call(args, ctx);
                if let Some(s) = saved {
                    eprintln!(
                        "hermes-rs denylist: undo copy tool={name} id={} files={} session={}",
                        s.id, s.files, ctx.session_id
                    );
                    attach_undo(&mut out, &s);
                }
                out
            }
            Err(d) => {
                eprintln!(
                    "hermes-rs denylist: refused tool={name} rule={} session={} depth={}",
                    d.rule, ctx.session_id, ctx.depth
                );
                ToolOutput::err(crate::denylist::refusal_text(&d))
            }
        }
    }
}

/// Tell the model where the undo copy is: an `undo` field in a JSON object
/// result, otherwise an `[undo]` line at the end.
fn attach_undo(out: &mut ToolOutput, s: &crate::denylist::undo::Saved) {
    if let Ok(Value::Object(mut o)) = serde_json::from_str::<Value>(&out.content) {
        if !o.contains_key("undo") {
            o.insert(
                "undo".into(),
                json!({"dir": s.dir.display().to_string(), "files": s.files, "note": s.note()}),
            );
            out.content = Value::Object(o).to_string();
            return;
        }
    }
    out.content.push_str("\n[undo] ");
    out.content.push_str(&s.note());
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// A registry whose tools use `policy` instead of the process-wide one.
    pub fn with_policy(policy: crate::denylist::Policy) -> Self {
        Registry {
            tools: Vec::new(),
            policy: Some(Arc::new(policy)),
        }
    }

    pub fn add(&mut self, t: Arc<dyn Tool>) {
        let n = t.name();
        self.tools.retain(|x| x.name() != n);
        let policy = self
            .policy
            .clone()
            .unwrap_or_else(crate::denylist::Policy::global);
        self.tools.push(Arc::new(Guarded { inner: t, policy }));
    }

    /// Add a tool whose paths resolve against (and may only write inside)
    /// `dir`, e.g. the coding tools' worktree. Same deny list otherwise.
    pub fn add_in_workspace(&mut self, t: Arc<dyn Tool>, dir: &std::path::Path) {
        let n = t.name();
        self.tools.retain(|x| x.name() != n);
        let base = self
            .policy
            .clone()
            .unwrap_or_else(crate::denylist::Policy::global);
        let policy = Arc::new(base.with_workspace(dir));
        self.tools.push(Arc::new(Guarded { inner: t, policy }));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn without(&self, name: &str) -> Registry {
        Registry {
            tools: self
                .tools
                .iter()
                .filter(|t| t.name() != name)
                .cloned()
                .collect(),
            policy: self.policy.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// OpenAI `tools` array.
    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name(), "description": t.description(), "parameters": t.parameters()}}))
            .collect()
    }
}

/// A trivial built-in so a node with no MCP still has one safe tool.
pub struct ClockTool;

impl Tool for ClockTool {
    fn name(&self) -> String {
        "clock".into()
    }
    fn description(&self) -> String {
        "Current time in Europe/Zagreb (RFC 3339 with offset).".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn call(&self, _args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::ok(crate::time::zagreb_rfc3339(crate::time::now_secs()))
    }
}

/// Sandbox run: a real `/bin/sh` terminal tool behind the guard, in a temp
/// workspace, full-write mode on.
#[cfg(test)]
mod full_write_sandbox_run {
    use super::*;

    struct Sh(std::path::PathBuf);
    impl Tool for Sh {
        fn name(&self) -> String {
            "terminal".into()
        }
        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }
        fn description(&self) -> String {
            "test shell".into()
        }
        fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
            let c = args["command"].as_str().unwrap_or_default();
            let o = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(c)
                .current_dir(&self.0)
                .output()
                .unwrap();
            let s = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            if o.status.success() {
                ToolOutput::ok(s)
            } else {
                ToolOutput::err(s)
            }
        }
    }

    fn run(reg: &Registry, c: &str) -> ToolOutput {
        let cancel = Arc::new(AtomicBool::new(false));
        let emit = |_e: Event| {};
        let ctx = ToolCtx {
            session_id: "t",
            spend_session: "t",
            depth: 0,
            user_text: "",
            cancel: &cancel,
            emit: &emit,
        };
        reg.get("terminal")
            .unwrap()
            .call(&json!({ "command": c }), &ctx)
    }

    #[test]
    fn write_delete_undo_shell_script_and_refusals() {
        let d = std::env::temp_dir().join(format!(
            "hermes-fw-sandbox-{}-{}",
            std::process::id(),
            crate::time::now_secs()
        ));
        std::fs::create_dir_all(&d).unwrap();
        let d = std::fs::canonicalize(&d).unwrap();
        let pol = crate::denylist::Policy::workspace(&d).full_write();
        let mut reg = Registry::with_policy(pol);
        reg.add(Arc::new(Sh(d.clone())));

        // allowed write
        let o = run(&reg, "mkdir -p src && echo 'hello' > src/a.txt");
        assert!(!o.is_error, "{}", o.content);
        assert_eq!(
            std::fs::read_to_string(d.join("src/a.txt")).unwrap(),
            "hello\n"
        );

        // delete with an undo copy, then restore it
        let o = run(&reg, "rm src/a.txt");
        assert!(!o.is_error, "{}", o.content);
        assert!(o.content.contains("[undo]"), "{}", o.content);
        assert!(!d.join("src/a.txt").exists());
        let trash = d.join(".hermes-trash");
        let id = std::fs::read_dir(&trash)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.join("src/a.txt").exists())
            .expect("undo copy");
        let st = std::process::Command::new("cp")
            .arg("-a")
            .arg(id.join("src/a.txt"))
            .arg(d.join("src/a.txt"))
            .status()
            .unwrap();
        assert!(st.success());
        assert_eq!(
            std::fs::read_to_string(d.join("src/a.txt")).unwrap(),
            "hello\n"
        );

        // overwrite keeps the old content too
        let o = run(&reg, "echo new > src/a.txt");
        assert!(!o.is_error && o.content.contains("[undo]"), "{}", o.content);

        // a shell command and a script
        let o = run(&reg, "ls src | wc -l > count.txt && cat count.txt");
        assert!(!o.is_error, "{}", o.content);
        std::fs::write(
            d.join("build.sh"),
            "#!/bin/sh\nset -e\nmkdir -p out\nfor f in src/*; do cp \"$f\" out/; done\necho built\n",
        )
        .unwrap();
        let o = run(&reg, "sh build.sh");
        assert!(!o.is_error && o.content.contains("built"), "{}", o.content);
        assert!(d.join("out/a.txt").exists());

        // refusals: nothing runs, nothing changes
        let before = std::fs::read_to_string(d.join("src/a.txt")).unwrap();
        for c in [
            "cat ~/.ssh/id_ed25519",
            "echo x > SOUL.md",
            "echo x >> /etc/hosts",
            "systemctl restart hermes-rs",
            "echo x > /tmp/outside-$$.txt",
            "rm -rf ../",
            "ln -s /etc etc2 && echo x > etc2/passwd",
            "sys''temctl stop x",
            "echo c3lzdGVtY3RsIHN0b3AgeA== | base64 -d | sh",
            "rm -rf .hermes-trash",
            "echo x > src/a.txt; rm -rf /",
        ] {
            let o = run(&reg, c);
            assert!(o.is_error, "expected refused: {c}\n{}", o.content);
            assert!(
                o.content.contains("refused") || o.content.contains("S"),
                "{c}: {}",
                o.content
            );
        }
        assert_eq!(
            std::fs::read_to_string(d.join("src/a.txt")).unwrap(),
            before
        );
        assert!(!d.join("SOUL.md").exists() && !d.join("etc2").exists());
        assert!(trash.exists());
    }
}
