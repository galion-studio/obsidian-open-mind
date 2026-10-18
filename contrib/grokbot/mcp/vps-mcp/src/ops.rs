//! Tool implementations shared by the MCP server and the CLI. Every op loads the
//! config fresh, connects, does its work read-only, and returns JSON.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::policy::{self, Category};
use crate::ssh::{Vps, sq};

const OUT_CAP: usize = 256 * 1024;

fn roots_or(cfg: &Config, roots: &Option<Vec<String>>) -> Vec<String> {
    match roots {
        Some(r) if !r.is_empty() => r.clone(),
        _ => cfg.roots.clone(),
    }
}

fn quoted_roots(roots: &[String]) -> String {
    roots.iter().map(|r| sq(r)).collect::<Vec<_>>().join(" ")
}

async fn connect(cfg: &Config) -> Result<Vps> {
    Vps::connect(cfg).await
}

fn timeout(cfg: &Config) -> Duration {
    Duration::from_secs(cfg.exec_timeout_secs)
}

fn sections(out: &str) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cur = String::from("_");
    for line in out.lines() {
        if let Some(name) = line.strip_prefix("###") {
            cur = name.trim().to_string();
            map.entry(cur.clone()).or_default();
        } else if !line.trim().is_empty() {
            map.entry(cur.clone()).or_default().push(line.to_string());
        }
    }
    map
}

fn join(v: Option<&Vec<String>>) -> String {
    v.map(|v| v.join("\n")).unwrap_or_default()
}

// ---------------------------------------------------------------- status
pub async fn status(cfg: &Config) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let vps = connect(cfg).await?;
    let connect_ms = t0.elapsed().as_millis();
    let script = "echo '###WHOAMI'; whoami; echo '###ID'; id; echo '###HOSTNAME'; hostname; \
echo '###UNAME'; uname -a; echo '###UPTIME'; uptime; echo '###LOADAVG'; cat /proc/loadavg 2>/dev/null; \
echo '###NPROC'; nproc 2>/dev/null; echo '###MEM'; free -m 2>/dev/null || cat /proc/meminfo; \
echo '###DISK'; df -h -x tmpfs -x devtmpfs -x overlay 2>/dev/null || df -h; \
echo '###OS'; grep -E '^(PRETTY_NAME|VERSION_ID)=' /etc/os-release 2>/dev/null";
    let r = vps.exec(&format!("sh -c {}", sq(script)), timeout(cfg), OUT_CAP).await?;
    let s = sections(&r.stdout);
    let v = json!({
        "connected": true,
        "target": format!("{}@{}:{}", vps.user, vps.host, cfg.port),
        "connect_ms": connect_ms,
        "host_key": {"sha256": vps.host_key.fingerprint, "status": vps.host_key.status, "known_hosts": cfg.known_hosts},
        "whoami": join(s.get("WHOAMI")),
        "id": join(s.get("ID")),
        "hostname": join(s.get("HOSTNAME")),
        "uname": join(s.get("UNAME")),
        "uptime": join(s.get("UPTIME")),
        "loadavg": join(s.get("LOADAVG")),
        "nproc": join(s.get("NPROC")),
        "memory": s.get("MEM").cloned().unwrap_or_default(),
        "disk": s.get("DISK").cloned().unwrap_or_default(),
        "os": s.get("OS").cloned().unwrap_or_default(),
        "exit_status": r.exit_status,
        "stderr": r.stderr,
    });
    vps.close().await;
    Ok(v)
}

// ---------------------------------------------------------------- exec
pub async fn exec(cfg: &Config, command: &str, allow_write: bool, timeout_secs: Option<u64>) -> Result<Value> {
    let mode = if allow_write {
        if !cfg.allow_write {
            bail!(
                "allow_write was requested but is disabled; set VPS_ALLOW_WRITE=true in {} to permit non-allowlisted commands",
                cfg.path.display()
            );
        }
        "write"
    } else {
        policy::check_readonly(command).map_err(|e| {
            anyhow!("read-only mode rejected the command: {e}. Allowed commands: {}", policy::READONLY_COMMANDS.join(", "))
        })?;
        "read-only"
    };
    let vps = connect(cfg).await?;
    let to = Duration::from_secs(timeout_secs.unwrap_or(cfg.exec_timeout_secs).clamp(1, 600));
    let r = vps.exec(command, to, OUT_CAP).await?;
    vps.close().await;
    Ok(json!({"mode": mode, "command": command, "result": r}))
}

// ---------------------------------------------------------------- list_dir / read_file
pub async fn list_dir(cfg: &Config, path: &str, show_hidden: bool) -> Result<Value> {
    let vps = connect(cfg).await?;
    let sftp = vps.sftp().await?;
    let canon = sftp.canonicalize(path).await.unwrap_or_else(|_| path.to_string());
    let mut entries = Vec::new();
    for e in sftp.read_dir(path).await.map_err(|e| anyhow!("read_dir {path}: {e}"))? {
        let name = e.file_name();
        if name == "." || name == ".." || (!show_hidden && name.starts_with('.')) {
            continue;
        }
        let m = e.metadata();
        let kind = if m.is_dir() { "dir" } else if m.is_symlink() { "symlink" } else if m.file_type().is_file() { "file" } else { "other" };
        let mtime = m.modified().ok().map(|t| humantime::format_rfc3339_seconds(t).to_string());
        entries.push(json!({"name": name, "type": kind, "size": m.size, "mtime": mtime}));
    }
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    vps.close().await;
    Ok(json!({"path": canon, "count": entries.len(), "entries": entries}))
}

pub async fn read_file(cfg: &Config, path: &str, offset: u64, max_bytes: Option<u64>) -> Result<Value> {
    let cap = max_bytes.unwrap_or(cfg.read_max_bytes).min(cfg.read_max_bytes).max(1);
    let vps = connect(cfg).await?;
    let (data, total) = vps.read_file(path, offset, cap).await?;
    vps.close().await;
    let truncated = total.map(|t| offset + (data.len() as u64) < t).unwrap_or(false);
    let binary = data.iter().take(8192).any(|b| *b == 0);
    Ok(json!({
        "path": path,
        "size": total,
        "offset": offset,
        "bytes_returned": data.len(),
        "truncated": truncated,
        "max_bytes": cap,
        "binary": binary,
        "content": if binary { Value::Null } else { Value::String(String::from_utf8_lossy(&data).into_owned()) },
        "note": if binary { "binary file; content omitted" } else if truncated { "truncated at size cap; use offset to page" } else { "" },
    }))
}

// ---------------------------------------------------------------- find
pub struct FindArgs {
    pub pattern: String,
    pub roots: Option<Vec<String>>,
    pub max_depth: Option<u32>,
    pub kind: Option<String>,
    pub limit: Option<u32>,
    pub case_sensitive: bool,
}

pub async fn find(cfg: &Config, a: FindArgs) -> Result<Value> {
    if a.pattern.trim().is_empty() {
        bail!("pattern is required");
    }
    let roots = roots_or(cfg, &a.roots);
    let depth = a.max_depth.unwrap_or(8).clamp(1, 30);
    let limit = a.limit.unwrap_or(500).clamp(1, 10000);
    let test = match (a.pattern.contains('/'), a.case_sensitive) {
        (true, true) => "-path",
        (true, false) => "-ipath",
        (false, true) => "-name",
        (false, false) => "-iname",
    };
    let kind = match a.kind.as_deref() {
        Some("f") | Some("file") => "-type f ",
        Some("d") | Some("dir") => "-type d ",
        _ => "",
    };
    let cmd = format!(
        "find {} -maxdepth {depth} {} -o {kind}{test} {} -print 2>/dev/null | head -n {}",
        quoted_roots(&roots), policy::PRUNE_EXPR, sq(&a.pattern), limit + 1
    );
    let vps = connect(cfg).await?;
    let r = vps.exec(&cmd, timeout(cfg), OUT_CAP).await?;
    vps.close().await;
    let mut paths: Vec<&str> = r.stdout.lines().filter(|l| !l.is_empty()).collect();
    let more = paths.len() > limit as usize;
    paths.truncate(limit as usize);
    Ok(json!({"pattern": a.pattern, "roots": roots, "max_depth": depth, "count": paths.len(), "limit_reached": more || r.truncated, "timed_out": r.timed_out, "paths": paths}))
}

// ---------------------------------------------------------------- discover_stack
const COMPONENTS: &[&str] = &["Galleon", "Galleon1", "Obsidian", "OpenMind"];
const NAME_RE: &str = "galleon|obsidian|open[-_ ]?mind";

fn components_of(line: &str) -> Vec<&'static str> {
    let l = line.to_ascii_lowercase();
    let mut v = Vec::new();
    let g1 = ["galleon1", "galleon-1", "galleon_1", "galleon 1"].iter().any(|p| l.contains(p));
    if g1 {
        v.push("Galleon1");
    }
    // plain Galleon: any galleon occurrence that is not only a galleon1 occurrence
    let stripped = ["galleon1", "galleon-1", "galleon_1", "galleon 1"].iter().fold(l.clone(), |acc, p| acc.replace(p, ""));
    if stripped.contains("galleon") {
        v.push("Galleon");
    }
    if l.contains("obsidian") {
        v.push("Obsidian");
    }
    if ["openmind", "open-mind", "open_mind", "open mind"].iter().any(|p| l.contains(p)) {
        v.push("OpenMind");
    }
    v
}

pub async fn discover_stack(cfg: &Config, roots: Option<Vec<String>>, max_depth: Option<u32>) -> Result<Value> {
    let mut roots = roots_or(cfg, &roots);
    for extra in ["/etc/systemd/system", "/lib/systemd/system"] {
        if !roots.iter().any(|r| r == extra) {
            roots.push(extra.to_string());
        }
    }
    let depth = max_depth.unwrap_or(5).clamp(1, 12);
    let r = quoted_roots(&roots);
    let p = policy::PRUNE_EXPR;
    let script = format!(
        "echo '###HOSTNAME'; hostname
echo '###PATHS'; find {r} -maxdepth {depth} {p} -o \\( -iname '*galleon*' -o -iname '*obsidian*' -o -iname '*openmind*' -o -iname '*open-mind*' -o -iname '*open_mind*' \\) -print 2>/dev/null | sort -u | head -n 500
echo '###UNITS'; if command -v systemctl >/dev/null 2>&1; then (systemctl list-units --all --no-pager --no-legend --plain --type=service,socket,timer 2>/dev/null; systemctl list-unit-files --no-pager --no-legend 2>/dev/null; systemctl --user list-units --all --no-pager --no-legend --plain 2>/dev/null) | grep -iE '{NAME_RE}' | sort -u | head -n 100; fi
echo '###UNITFILES'; grep -rilE '{NAME_RE}' /etc/systemd/system /lib/systemd/system /usr/lib/systemd/system $HOME/.config/systemd/user 2>/dev/null | sort -u | head -n 100
echo '###COMPOSE'; find {r} -maxdepth {depth} {p} -o -type f \\( -name 'docker-compose*.yml' -o -name 'docker-compose*.yaml' -o -name 'compose*.yml' -o -name 'compose*.yaml' \\) -print 2>/dev/null | sort -u | head -n 200
echo '###COMPOSE_MATCH'; find {r} -maxdepth {depth} {p} -o -type f \\( -name 'docker-compose*.yml' -o -name 'docker-compose*.yaml' -o -name 'compose*.yml' -o -name 'compose*.yaml' \\) -exec grep -ilE '{NAME_RE}' {{}} + 2>/dev/null | sort -u | head -n 200
echo '###DOCKER'; if command -v docker >/dev/null 2>&1; then docker ps -a --format '{{{{.Names}}}}\t{{{{.Image}}}}\t{{{{.Status}}}}\t{{{{.Ports}}}}' 2>&1 | head -n 200; else echo 'docker: not installed'; fi
echo '###PORTS'; (ss -tulnpH 2>/dev/null || netstat -tulnp 2>/dev/null) | head -n 300
echo '###PROCS'; ps -eo pid,user,args 2>/dev/null | grep -iE '{NAME_RE}' | grep -v -e grep -e '###' | head -n 100
echo '###END'"
    );
    let vps = connect(cfg).await?;
    let out = vps.exec(&format!("sh -c {}", sq(&script)), Duration::from_secs(cfg.exec_timeout_secs.max(120)), OUT_CAP).await?;
    let host_key = vps.host_key.clone();
    vps.close().await;
    let s = sections(&out.stdout);
    let get = |k: &str| s.get(k).cloned().unwrap_or_default();

    let mut comps: BTreeMap<&str, BTreeMap<&str, Vec<String>>> = BTreeMap::new();
    for c in COMPONENTS {
        let mut m = BTreeMap::new();
        for f in ["paths", "systemd_units", "systemd_unit_files", "compose_files", "containers", "listening_ports", "processes"] {
            m.insert(f, Vec::new());
        }
        comps.insert(c, m);
    }
    let mut add = |section: &str, field: &'static str| {
        for line in get(section) {
            for c in components_of(&line) {
                comps.get_mut(c).unwrap().get_mut(field).unwrap().push(line.clone());
            }
        }
    };
    add("PATHS", "paths");
    add("UNITS", "systemd_units");
    add("UNITFILES", "systemd_unit_files");
    add("COMPOSE_MATCH", "compose_files");
    add("DOCKER", "containers");
    add("PORTS", "listening_ports");
    add("PROCS", "processes");
    // Compose files whose *path* mentions a component are also relevant.
    for line in get("COMPOSE") {
        for c in components_of(&line) {
            let v = comps.get_mut(c).unwrap().get_mut("compose_files").unwrap();
            if !v.contains(&line) {
                v.push(line.clone());
            }
        }
    }
    let summary: BTreeMap<&str, Value> = comps
        .iter()
        .map(|(c, m)| {
            let found = m.values().any(|v| !v.is_empty());
            let mut obj = serde_json::Map::new();
            obj.insert("found".into(), json!(found));
            for (k, v) in m {
                obj.insert(k.to_string(), json!(v));
            }
            (*c, Value::Object(obj))
        })
        .collect();
    Ok(json!({
        "host": join(s.get("HOSTNAME")),
        "host_key": {"sha256": host_key.fingerprint, "status": host_key.status},
        "roots_searched": roots,
        "max_depth": depth,
        "components": summary,
        "all_compose_files": get("COMPOSE"),
        "docker": get("DOCKER"),
        "listening_ports": get("PORTS"),
        "timed_out": out.timed_out,
        "complete": s.contains_key("END"),
        "stderr": out.stderr,
    }))
}

// ---------------------------------------------------------------- agent files
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentFile {
    pub path: String,
    pub category: Category,
    pub size: Option<u64>,
    pub secret: Option<&'static str>,
}

async fn find_agent_files_on(vps: &Vps, cfg: &Config, roots: &[String], depth: u32, limit: u32) -> Result<(Vec<AgentFile>, bool)> {
    let cmd = format!(
        "find {} -maxdepth {depth} {} -o {} -print 2>/dev/null | head -n {}",
        quoted_roots(roots), policy::PRUNE_EXPR, policy::AGENT_FIND_EXPR, limit + 1
    );
    let r = vps.exec(&cmd, Duration::from_secs(cfg.exec_timeout_secs.max(120)), 4 * 1024 * 1024).await?;
    let mut paths: Vec<String> = r.stdout.lines().filter(|l| !l.is_empty()).map(String::from).collect();
    paths.sort();
    paths.dedup();
    let more = paths.len() > limit as usize;
    paths.truncate(limit as usize);
    let sftp = vps.sftp().await?;
    let mut out = Vec::new();
    for p in paths {
        let size = sftp.metadata(p.as_str()).await.ok().and_then(|m| m.size);
        out.push(AgentFile { category: policy::classify(&p), secret: policy::secret_reason(&p), size, path: p });
    }
    Ok((out, more || r.truncated || r.timed_out))
}

pub async fn find_agent_files(cfg: &Config, roots: Option<Vec<String>>, max_depth: Option<u32>, limit: Option<u32>) -> Result<Value> {
    let roots = roots_or(cfg, &roots);
    let depth = max_depth.unwrap_or(8).clamp(1, 30);
    let limit = limit.unwrap_or(5000).clamp(1, 50000);
    let vps = connect(cfg).await?;
    let (files, partial) = find_agent_files_on(&vps, cfg, &roots, depth, limit).await?;
    vps.close().await;
    let count = |c: Category| files.iter().filter(|f| f.category == c).count();
    Ok(json!({
        "roots": roots,
        "max_depth": depth,
        "partial": partial,
        "counts": {"persona": count(Category::Persona), "skill": count(Category::Skill), "workflow": count(Category::Workflow), "other": count(Category::Other),
                   "secret_like": files.iter().filter(|f| f.secret.is_some()).count()},
        "files": files,
    }))
}

pub struct PullArgs {
    pub roots: Option<Vec<String>>,
    pub max_depth: Option<u32>,
    pub paths: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
    pub include_secrets: bool,
    pub overwrite: bool,
    pub dry_run: bool,
}

fn sha256_hex(d: &[u8]) -> String {
    hex::encode(Sha256::digest(d))
}

fn local_dest(cfg: &Config, cat: Category, remote: &str) -> Result<PathBuf> {
    let rel = remote.trim_start_matches('/');
    if rel.is_empty() || rel.split('/').any(|c| c == ".." || c == ".") {
        bail!("unsafe remote path {remote}");
    }
    let host = cfg.host_label();
    let base = match cat {
        Category::Skill => cfg.skills_import_dir.join(&host),
        Category::Persona => cfg.skills_import_dir.join(&host).join("persona"),
        Category::Workflow => cfg.workflows_import_dir.join(&host),
        Category::Other => cfg.skills_import_dir.join(&host).join("other"),
    };
    Ok(base.join(rel))
}

fn write_manifest(path: &Path, v: &Value) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

pub async fn pull_agent_files(cfg: &Config, a: PullArgs) -> Result<Value> {
    let roots = roots_or(cfg, &a.roots);
    let depth = a.max_depth.unwrap_or(8).clamp(1, 30);
    let vps = connect(cfg).await?;
    let (mut files, partial) = match &a.paths {
        Some(p) if !p.is_empty() => {
            let sftp = vps.sftp().await?;
            let mut v = Vec::new();
            for path in p {
                let size = sftp.metadata(path.as_str()).await.ok().and_then(|m| m.size);
                v.push(AgentFile { category: policy::classify(path), secret: policy::secret_reason(path), size, path: path.clone() });
            }
            (v, false)
        }
        _ => find_agent_files_on(&vps, cfg, &roots, depth, 20000).await?,
    };
    if let Some(cats) = &a.categories {
        let cats: Vec<String> = cats.iter().map(|c| c.to_ascii_lowercase()).collect();
        files.retain(|f| cats.iter().any(|c| serde_json::to_value(f.category).unwrap() == json!(c)));
    }
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    let sftp = vps.sftp().await?;
    let (mut pulled, mut skipped) = (Vec::new(), Vec::new());
    for f in &files {
        let skip = |reason: String| json!({"path": f.path, "category": f.category, "size": f.size, "reason": reason});
        if let (Some(r), false) = (f.secret, a.include_secrets) {
            skipped.push(skip(format!("secret-like ({r}); pass include_secrets=true to copy")));
            continue;
        }
        match f.size {
            Some(s) if s > cfg.pull_max_file_bytes => {
                skipped.push(skip(format!("larger than VPS_PULL_MAX_FILE_BYTES={}", cfg.pull_max_file_bytes)));
                continue;
            }
            None => {
                skipped.push(skip("cannot stat (permission denied or vanished)".into()));
                continue;
            }
            _ => {}
        }
        let dest = match local_dest(cfg, f.category, &f.path) {
            Ok(d) => d,
            Err(e) => {
                skipped.push(skip(e.to_string()));
                continue;
            }
        };
        if a.dry_run {
            pulled.push(json!({"category": f.category, "remote_path": f.path, "local_path": dest, "size": f.size, "status": "dry-run"}));
            continue;
        }
        let data = match sftp.read(f.path.as_str()).await {
            Ok(d) => d,
            Err(e) => {
                skipped.push(skip(format!("read failed: {e}")));
                continue;
            }
        };
        if let (Some(r), false) = (policy::secret_in_content(&data), a.include_secrets) {
            skipped.push(skip(format!("secret content ({r}); pass include_secrets=true to copy")));
            continue;
        }
        let sha = sha256_hex(&data);
        let mut status = "written";
        if dest.exists() {
            let existing = std::fs::read(&dest).unwrap_or_default();
            if sha256_hex(&existing) == sha {
                status = "unchanged";
            } else if !a.overwrite {
                skipped.push(skip(format!("local file {} exists with different content; pass overwrite=true", dest.display())));
                continue;
            } else {
                status = "overwritten";
            }
        }
        if status != "unchanged" {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = dest.with_extension("vps-mcp-tmp");
            std::fs::write(&tmp, &data)?;
            std::fs::rename(&tmp, &dest)?;
        }
        pulled.push(json!({
            "category": f.category, "remote_path": f.path, "local_path": dest,
            "size": data.len(), "sha256": sha, "pulled_at": now, "status": status,
        }));
    }
    vps.close().await;
    let host = cfg.host_label();
    let manifest = json!({
        "tool": "vps-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "source": format!("{}@{}:{}", cfg.user.clone().unwrap_or_default(), cfg.host.clone().unwrap_or_default(), cfg.port),
        "timestamp": now,
        "roots": roots,
        "partial_listing": partial,
        "dry_run": a.dry_run,
        "files": pulled,
        "skipped": skipped,
    });
    let mut manifests = Vec::new();
    if !a.dry_run {
        for base in [cfg.skills_import_dir.join(&host), cfg.workflows_import_dir.join(&host)] {
            let m = base.join("manifest.json");
            write_manifest(&m, &manifest)?;
            manifests.push(m);
        }
    }
    Ok(json!({
        "pulled": manifest["files"].as_array().map(|v| v.len()).unwrap_or(0),
        "skipped": manifest["skipped"].as_array().map(|v| v.len()).unwrap_or(0),
        "manifests": manifests,
        "destinations": {
            "skills": cfg.skills_import_dir.join(&host),
            "persona": cfg.skills_import_dir.join(&host).join("persona"),
            "workflows": cfg.workflows_import_dir.join(&host),
        },
        "manifest": manifest,
    }))
}
