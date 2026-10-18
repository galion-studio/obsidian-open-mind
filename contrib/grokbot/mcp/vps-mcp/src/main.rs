//! vps-mcp: MCP stdio server (and CLI) for read-only access to a remote VPS over SSH.

mod config;
mod ops;
mod policy;
mod ssh;

use std::path::PathBuf;

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::Value;

use config::Config;

#[derive(Clone)]
#[allow(dead_code)]
struct VpsServer {
    config_path: PathBuf,
    tool_router: ToolRouter<Self>,
}

fn respond(r: anyhow::Result<Value>) -> Result<CallToolResult, McpError> {
    Ok(match r {
        Ok(v) => CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&v).unwrap_or_default())]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("error: {e:#}"))]),
    })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExecParams {
    /// Command to run on the VPS. In read-only mode it must use allowlisted commands (pipes allowed; no ; & > < $ `).
    command: String,
    /// Request write mode (bypasses the allowlist). Only honored if VPS_ALLOW_WRITE=true in the config file.
    #[serde(default)]
    allow_write: Option<bool>,
    /// Timeout in seconds (default VPS_EXEC_TIMEOUT or 60, max 600).
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListDirParams {
    /// Remote directory path (absolute, or relative to the SSH user's home).
    path: String,
    /// Include dotfiles (default false).
    #[serde(default)]
    show_hidden: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReadFileParams {
    /// Remote file path.
    path: String,
    /// Byte offset to start at (default 0).
    #[serde(default)]
    offset: Option<u64>,
    /// Max bytes to return (capped by VPS_READ_MAX_BYTES, default 512 KiB).
    #[serde(default)]
    max_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindParams {
    /// Glob for the file name (e.g. "*.yaml", "SKILL.md"); if it contains '/', it matches the full path.
    pattern: String,
    /// Roots to search (default VPS_PROJECT_ROOTS or /opt /srv /home /var/www /root /etc/systemd /usr/local).
    #[serde(default)]
    roots: Option<Vec<String>>,
    /// Max directory depth (default 8).
    #[serde(default)]
    max_depth: Option<u32>,
    /// "f" for files, "d" for directories, omitted for any.
    #[serde(default)]
    kind: Option<String>,
    /// Max results (default 500).
    #[serde(default)]
    limit: Option<u32>,
    /// Case-sensitive match (default false).
    #[serde(default)]
    case_sensitive: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DiscoverParams {
    /// Roots to search (default VPS_PROJECT_ROOTS or common roots).
    #[serde(default)]
    roots: Option<Vec<String>>,
    /// Max depth for the file search (default 5).
    #[serde(default)]
    max_depth: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindAgentParams {
    /// Roots to search (default VPS_PROJECT_ROOTS or common roots).
    #[serde(default)]
    roots: Option<Vec<String>>,
    /// Max depth (default 8).
    #[serde(default)]
    max_depth: Option<u32>,
    /// Max files to list (default 5000).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PullParams {
    /// Roots to search (default VPS_PROJECT_ROOTS or common roots). Ignored if `paths` is given.
    #[serde(default)]
    roots: Option<Vec<String>>,
    /// Max depth (default 8).
    #[serde(default)]
    max_depth: Option<u32>,
    /// Explicit remote file paths to pull instead of auto-discovery.
    #[serde(default)]
    paths: Option<Vec<String>>,
    /// Only these categories: any of "persona", "skill", "workflow".
    #[serde(default)]
    categories: Option<Vec<String>>,
    /// Also copy secret-like files (.env, private keys, credentials). Default false.
    #[serde(default)]
    include_secrets: Option<bool>,
    /// Overwrite previously imported local files whose content changed. Default false.
    #[serde(default)]
    overwrite: Option<bool>,
    /// List what would be copied without copying. Default false.
    #[serde(default)]
    dry_run: Option<bool>,
}

#[tool_router]
impl VpsServer {
    fn new(config_path: PathBuf) -> Self {
        Self { config_path, tool_router: Self::tool_router() }
    }

    fn cfg(&self) -> Config {
        Config::load(&self.config_path)
    }

    #[tool(description = "Show the resolved vps-mcp configuration (config file path, host/user, port, key path, roots, write mode). Does not connect.")]
    async fn vps_config(&self) -> Result<CallToolResult, McpError> {
        respond(Ok(self.cfg().summary()))
    }

    #[tool(description = "Test the SSH connection to the VPS and return whoami, uname, uptime, load, memory, disk, OS and host-key status.")]
    async fn vps_status(&self) -> Result<CallToolResult, McpError> {
        respond(ops::status(&self.cfg()).await)
    }

    #[tool(description = "Run a shell command on the VPS. Read-only allowlist by default (ls, cat, grep, find, ps, ss, systemctl status, docker ps/logs/inspect, journalctl, git log/status, ...). allow_write=true bypasses the allowlist only if VPS_ALLOW_WRITE=true in the config.")]
    async fn vps_exec(&self, Parameters(p): Parameters<ExecParams>) -> Result<CallToolResult, McpError> {
        respond(ops::exec(&self.cfg(), &p.command, p.allow_write.unwrap_or(false), p.timeout_secs).await)
    }

    #[tool(description = "List a remote directory over SFTP (name, type, size, mtime).")]
    async fn vps_list_dir(&self, Parameters(p): Parameters<ListDirParams>) -> Result<CallToolResult, McpError> {
        respond(ops::list_dir(&self.cfg(), &p.path, p.show_hidden.unwrap_or(false)).await)
    }

    #[tool(description = "Read a remote text file over SFTP, with a size cap (VPS_READ_MAX_BYTES) and optional offset for paging.")]
    async fn vps_read_file(&self, Parameters(p): Parameters<ReadFileParams>) -> Result<CallToolResult, McpError> {
        respond(ops::read_file(&self.cfg(), &p.path, p.offset.unwrap_or(0), p.max_bytes).await)
    }

    #[tool(description = "Find files/dirs on the VPS by name or path glob under the project roots (skips node_modules, .git, venvs, caches).")]
    async fn vps_find(&self, Parameters(p): Parameters<FindParams>) -> Result<CallToolResult, McpError> {
        respond(
            ops::find(&self.cfg(), ops::FindArgs {
                pattern: p.pattern,
                roots: p.roots,
                max_depth: p.max_depth,
                kind: p.kind,
                limit: p.limit,
                case_sensitive: p.case_sensitive.unwrap_or(false),
            })
            .await,
        )
    }

    #[tool(description = "Locate Galleon, Galleon1, Obsidian and OpenMind on the VPS: matching paths under common roots, systemd units/unit files, docker compose files, docker containers, listening ports and processes. Returns a structured per-component summary.")]
    async fn vps_discover_stack(&self, Parameters(p): Parameters<DiscoverParams>) -> Result<CallToolResult, McpError> {
        respond(ops::discover_stack(&self.cfg(), p.roots, p.max_depth).await)
    }

    #[tool(description = "Find agent files on the VPS: persona files (persona*, SOUL*, *system_prompt*, *.persona.*), skill files (SKILL.md, skills/ dirs) and workflow files (workflows/ dirs, *.workflow.*). Flags secret-like files.")]
    async fn vps_find_agent_files(&self, Parameters(p): Parameters<FindAgentParams>) -> Result<CallToolResult, McpError> {
        respond(ops::find_agent_files(&self.cfg(), p.roots, p.max_depth, p.limit).await)
    }

    #[tool(description = "Copy persona/skill/workflow files from the VPS via SFTP into ~/skills/imported-from-vps/<host>/ (persona under .../persona/) and ~/workflows/imported-from-vps/<host>/, preserving remote paths, and write manifest.json (path, size, sha256, timestamp). Secret-like files are skipped unless include_secrets=true. Never writes to the VPS.")]
    async fn vps_pull_agent_files(&self, Parameters(p): Parameters<PullParams>) -> Result<CallToolResult, McpError> {
        respond(
            ops::pull_agent_files(&self.cfg(), ops::PullArgs {
                roots: p.roots,
                max_depth: p.max_depth,
                paths: p.paths,
                categories: p.categories,
                include_secrets: p.include_secrets.unwrap_or(false),
                overwrite: p.overwrite.unwrap_or(false),
                dry_run: p.dry_run.unwrap_or(false),
            })
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for VpsServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vps-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(format!(
                "Read-only SSH/SFTP access to the collaborator VPS. Config: {}. Start with vps_status, then vps_discover_stack / vps_find_agent_files, then vps_pull_agent_files.",
                self.config_path.display()
            ))
    }
}

const USAGE: &str = "vps-mcp — MCP stdio server + CLI for read-only VPS access over SSH (pure Rust)

USAGE: vps-mcp [--config PATH] [COMMAND]

COMMANDS:
  serve                         Run the MCP stdio server (default when no command given)
  config                        Print the resolved configuration
  init-config [PATH]            Write a placeholder config file if it does not exist
  status                        Connection test + system summary
  discover [ROOT...]            Locate Galleon/Galleon1/Obsidian/OpenMind
  find-agent-files [ROOT...]    List persona/skill/workflow files
  pull [--include-secrets] [--overwrite] [--dry-run] [ROOT...]
                                Copy agent files to ~/skills|~/workflows/imported-from-vps/<host>/
  exec [--allow-write] -- CMD   Run a command (read-only allowlist unless --allow-write and VPS_ALLOW_WRITE=true)
  ls PATH | cat PATH | find GLOB [ROOT...]

Config path: --config > $VPS_MCP_CONFIG > ~/workflows/vps/config.env
";

async fn cli(cfg: Config, cmd: &str, rest: Vec<String>) -> i32 {
    let roots = |r: &[String]| -> Option<Vec<String>> { if r.is_empty() { None } else { Some(r.to_vec()) } };
    let flags: Vec<&String> = rest.iter().filter(|a| a.starts_with("--")).collect();
    let pos: Vec<String> = rest.iter().filter(|a| !a.starts_with("--")).cloned().collect();
    let flag = |f: &str| flags.iter().any(|a| *a == f);
    let r = match cmd {
        "config" => Ok(cfg.summary()),
        "status" => ops::status(&cfg).await,
        "discover" => ops::discover_stack(&cfg, roots(&pos), None).await,
        "find-agent-files" => ops::find_agent_files(&cfg, roots(&pos), None, None).await,
        "pull" => {
            ops::pull_agent_files(&cfg, ops::PullArgs {
                roots: roots(&pos),
                max_depth: None,
                paths: None,
                categories: None,
                include_secrets: flag("--include-secrets"),
                overwrite: flag("--overwrite"),
                dry_run: flag("--dry-run"),
            })
            .await
        }
        "exec" => {
            let idx = rest.iter().position(|a| a == "--");
            let command = match idx {
                Some(i) => rest[i + 1..].join(" "),
                None => pos.join(" "),
            };
            ops::exec(&cfg, &command, flag("--allow-write"), None).await
        }
        "ls" => ops::list_dir(&cfg, pos.first().map(|s| s.as_str()).unwrap_or("."), flag("--all")).await,
        "cat" => match pos.first() {
            Some(p) => ops::read_file(&cfg, p, 0, None).await,
            None => Err(anyhow::anyhow!("usage: vps-mcp cat PATH")),
        },
        "find" => match pos.split_first() {
            Some((pat, r)) => {
                ops::find(&cfg, ops::FindArgs { pattern: pat.clone(), roots: roots(r), max_depth: None, kind: None, limit: None, case_sensitive: false }).await
            }
            None => Err(anyhow::anyhow!("usage: vps-mcp find GLOB [ROOT...]")),
        },
        other => {
            eprintln!("unknown command '{other}'\n\n{USAGE}");
            return 2;
        }
    };
    match r {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            0
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut config_flag = None;
    if let Some(i) = args.iter().position(|a| a == "--config" || a == "-c") {
        if i + 1 < args.len() {
            config_flag = Some(args.remove(i + 1));
            args.remove(i);
        }
    }
    let path = config::resolve_config_path(config_flag.as_deref());
    let cmd = if args.is_empty() { "serve".to_string() } else { args.remove(0) };
    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
        }
        "-V" | "--version" => println!("vps-mcp {}", env!("CARGO_PKG_VERSION")),
        "init-config" => {
            let p = args.first().map(|s| config::expand_path(s)).unwrap_or(path);
            if p.exists() {
                println!("{} already exists; not modified", p.display());
            } else {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match std::fs::write(&p, config::TEMPLATE) {
                    Ok(()) => println!("wrote placeholder config to {}", p.display()),
                    Err(e) => {
                        eprintln!("error writing {}: {e}", p.display());
                        std::process::exit(1);
                    }
                }
            }
        }
        "serve" => {
            eprintln!("vps-mcp {}: MCP stdio server starting (config: {})", env!("CARGO_PKG_VERSION"), path.display());
            let server = match VpsServer::new(path).serve(rmcp::transport::stdio()).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("vps-mcp: failed to start MCP server: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = server.waiting().await {
                eprintln!("vps-mcp: server error: {e}");
            }
        }
        other => {
            let code = cli(Config::load(&path), other, args).await;
            std::process::exit(code);
        }
    }
}
