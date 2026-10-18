//! Pure-Rust SSH/SFTP (russh + russh-sftp). Public-key auth with the configured key,
//! trust-on-first-use host keys stored in a dedicated known_hosts file.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::io::AsyncReadExt;

use crate::config::Config;

#[derive(Default, Clone, Debug)]
pub struct HostKeyInfo {
    pub fingerprint: String,
    pub status: String, // "known" | "learned" | refusal reason
}

struct Client {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    info: Arc<Mutex<HostKeyInfo>>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(_) => {
                self.info.lock().unwrap().status = "refused: host certificates are not supported".into();
                return Ok(false);
            }
        };
        let fp = key.fingerprint(HashAlg::Sha256).to_string();
        let mut info = self.info.lock().unwrap();
        info.fingerprint = fp.clone();
        let recorded = if self.known_hosts.exists() {
            match russh::keys::known_hosts::known_host_keys_path(&self.host, self.port, &self.known_hosts) {
                Ok(v) => v,
                Err(e) => {
                    info.status = format!("refused: cannot parse known_hosts {}: {e}", self.known_hosts.display());
                    return Ok(false);
                }
            }
        } else {
            vec![]
        };
        if recorded.is_empty() {
            // Trust on first use.
            match russh::keys::known_hosts::learn_known_hosts_path(&self.host, self.port, &key, &self.known_hosts) {
                Ok(()) => {
                    info.status = format!("learned (TOFU) and saved to {}", self.known_hosts.display());
                    Ok(true)
                }
                Err(e) => {
                    info.status = format!("refused: could not record host key in {}: {e}", self.known_hosts.display());
                    Ok(false)
                }
            }
        } else if recorded.iter().any(|(_, k)| *k == key) {
            info.status = "known (matches known_hosts)".into();
            Ok(true)
        } else {
            let lines: Vec<String> = recorded.iter().map(|(l, _)| l.to_string()).collect();
            info.status = format!(
                "refused: HOST KEY MISMATCH for {}:{} (server offered {fp}; known_hosts {} line(s) {} record a different key). Possible MITM or server reinstall; verify out-of-band, then remove the old line to re-trust.",
                self.host,
                self.port,
                self.known_hosts.display(),
                lines.join(",")
            );
            Ok(false)
        }
    }
}

pub struct Vps {
    handle: client::Handle<Client>,
    pub host: String,
    pub user: String,
    pub host_key: HostKeyInfo,
    sftp: tokio::sync::OnceCell<SftpSession>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecOut {
    pub exit_status: Option<u32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub timed_out: bool,
}

impl Vps {
    pub async fn connect(cfg: &Config) -> Result<Vps> {
        let (host, user) = cfg.target().map_err(|e| anyhow!(e))?;
        if !cfg.key.exists() {
            bail!("SSH key {} not found", cfg.key.display());
        }
        let key = russh::keys::load_secret_key(&cfg.key, None)
            .with_context(|| format!("loading SSH key {}", cfg.key.display()))?;
        let info = Arc::new(Mutex::new(HostKeyInfo::default()));
        let handler = Client {
            host: host.clone(),
            port: cfg.port,
            known_hosts: cfg.known_hosts.clone(),
            info: info.clone(),
        };
        let ssh_cfg = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(cfg.exec_timeout_secs.max(60) + 30)),
            keepalive_interval: Some(Duration::from_secs(20)),
            ..Default::default()
        });
        let to = Duration::from_secs(cfg.connect_timeout_secs);
        let res = tokio::time::timeout(to, client::connect(ssh_cfg, (host.as_str(), cfg.port), handler)).await;
        let host_key = info.lock().unwrap().clone();
        let mut handle = match res {
            Err(_) => bail!("timed out connecting to {}:{} after {}s", host, cfg.port, cfg.connect_timeout_secs),
            Ok(Err(e)) => {
                if host_key.status.starts_with("refused") {
                    bail!("{}", host_key.status);
                }
                bail!("SSH connect to {}:{} failed: {e}", host, cfg.port)
            }
            Ok(Ok(h)) => h,
        };
        let is_rsa = matches!(key.algorithm(), russh::keys::Algorithm::Rsa { .. });
        let hash = if is_rsa { handle.best_supported_rsa_hash().await?.flatten() } else { None };
        let auth = tokio::time::timeout(
            to,
            handle.authenticate_publickey(user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), hash)),
        )
        .await
        .map_err(|_| anyhow!("timed out during SSH authentication"))??;
        if !auth.success() {
            bail!("SSH public-key authentication failed for {user}@{host} with {}", cfg.key.display());
        }
        Ok(Vps { handle, host, user, host_key, sftp: tokio::sync::OnceCell::new() })
    }

    pub async fn exec(&self, command: &str, timeout: Duration, cap: usize) -> Result<ExecOut> {
        let mut ch = self.handle.channel_open_session().await?;
        ch.exec(true, command).await?;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut exit_status = None;
        let mut truncated = false;
        let fut = async {
            while let Some(msg) = ch.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        if out.len() < cap { out.extend_from_slice(data) } else { truncated = true }
                    }
                    ChannelMsg::ExtendedData { ref data, .. } => {
                        if err.len() < cap { err.extend_from_slice(data) } else { truncated = true }
                    }
                    ChannelMsg::ExitStatus { exit_status: s } => exit_status = Some(s),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
        };
        let timed_out = tokio::time::timeout(timeout, fut).await.is_err();
        if out.len() > cap { out.truncate(cap); truncated = true; }
        if err.len() > cap { err.truncate(cap); truncated = true; }
        Ok(ExecOut {
            exit_status,
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            truncated,
            timed_out,
        })
    }

    pub async fn sftp(&self) -> Result<&SftpSession> {
        self.sftp
            .get_or_try_init(|| async {
                let ch = self.handle.channel_open_session().await?;
                ch.request_subsystem(true, "sftp").await?;
                let s = SftpSession::new(ch.into_stream()).await.context("starting SFTP subsystem")?;
                Ok::<_, anyhow::Error>(s)
            })
            .await
    }

    /// Read up to `max` bytes starting at `offset`. Returns (bytes, total_size).
    pub async fn read_file(&self, path: &str, offset: u64, max: u64) -> Result<(Vec<u8>, Option<u64>)> {
        use tokio::io::AsyncSeekExt;
        let sftp = self.sftp().await?;
        let meta = sftp.metadata(path).await.with_context(|| format!("stat {path}"))?;
        if meta.is_dir() {
            bail!("{path} is a directory");
        }
        let mut f = sftp.open(path).await.with_context(|| format!("open {path}"))?;
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset)).await?;
        }
        let mut buf = Vec::new();
        (&mut f).take(max).read_to_end(&mut buf).await?;
        Ok((buf, meta.size))
    }

    pub async fn close(self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
    }
}

/// POSIX shell single-quote.
pub fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
