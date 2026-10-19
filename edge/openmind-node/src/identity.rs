// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Device identity: an ed25519 keypair that never leaves the device, plus the
//! list of trusted networks (salted hashes only) it has been bound to.
//!
//! Files (in ~/.config/openmind, dir mode 700):
//!   node.key         32-byte secret seed, hex, mode 600 (never printed)
//!   node-state.toml  public key, device id, salt, trusted network hashes

use crate::config::{config_dir, Config};
use crate::util::{hex, random_bytes, unhex};
use crate::{wifi, zagreb};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustedNetwork {
    pub hash: String,
    pub anchor: String,
    pub bound_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub device_id: String,
    pub public_key: String,
    pub created: String,
    /// Per-device salt for network hashes (used unless config sets a stack salt).
    pub salt_hex: String,
    #[serde(default)]
    pub trusted_networks: Vec<TrustedNetwork>,
}

pub struct Identity {
    pub key: SigningKey,
    pub state: State,
    dir: PathBuf,
}

pub fn fingerprint(vk: &VerifyingKey) -> String {
    let d = Sha256::digest(vk.as_bytes());
    let h = hex(&d[..16]);
    let groups: Vec<&str> = (0..h.len()).step_by(4).map(|i| &h[i..i + 4]).collect();
    format!("SHA256:{}", groups.join("-"))
}

pub fn device_id(vk: &VerifyingKey) -> String {
    format!("omn-{}", &hex(&Sha256::digest(vk.as_bytes()))[..12])
}

fn key_path(dir: &Path) -> PathBuf {
    dir.join("node.key")
}
fn state_path(dir: &Path) -> PathBuf {
    dir.join("node-state.toml")
}

impl Identity {
    pub fn dir_default() -> PathBuf {
        config_dir()
    }

    /// Create a new identity. Refuses to overwrite an existing key.
    pub fn init(dir: &Path) -> Result<Identity, String> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("create {}: {e}", dir.display()))?;
        let kp = key_path(dir);
        if kp.exists() {
            return Err(format!(
                "{} already exists; refusing to overwrite a device key",
                kp.display()
            ));
        }
        let seed = random_bytes::<32>().map_err(|e| e.to_string())?;
        let key = SigningKey::from_bytes(&seed);
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&kp)
            .map_err(|e| format!("create {}: {e}", kp.display()))?;
        f.write_all(format!("{}\n", hex(&seed)).as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_all().ok();
        let vk = key.verifying_key();
        let state = State {
            device_id: device_id(&vk),
            public_key: hex(vk.as_bytes()),
            created: zagreb::now_rfc3339(),
            salt_hex: hex(&random_bytes::<16>().map_err(|e| e.to_string())?),
            trusted_networks: vec![],
        };
        let id = Identity {
            key,
            state,
            dir: dir.to_path_buf(),
        };
        id.save()?;
        Ok(id)
    }

    pub fn load(dir: &Path) -> Result<Identity, String> {
        let kp = key_path(dir);
        let meta = fs::metadata(&kp).map_err(|_| {
            format!(
                "no identity at {} (run `omnode identity init`)",
                kp.display()
            )
        })?;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "{} is readable by group/other; fix with chmod 600",
                kp.display()
            ));
        }
        let txt = fs::read_to_string(&kp).map_err(|e| e.to_string())?;
        let seed: [u8; 32] = unhex(&txt)
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| format!("{} is not a valid key file", kp.display()))?;
        let key = SigningKey::from_bytes(&seed);
        let st_txt =
            fs::read_to_string(state_path(dir)).map_err(|e| format!("node-state.toml: {e}"))?;
        let state: State = toml::from_str(&st_txt).map_err(|e| format!("node-state.toml: {e}"))?;
        if state.public_key != hex(key.verifying_key().as_bytes()) {
            return Err("node-state.toml public key does not match node.key".into());
        }
        Ok(Identity {
            key,
            state,
            dir: dir.to_path_buf(),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let p = state_path(&self.dir);
        let tmp = p.with_extension("toml.tmp");
        let body = toml::to_string(&self.state).map_err(|e| e.to_string())?;
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.verifying_key())
    }

    pub fn sign(&self, msg: &[u8]) -> String {
        hex(&self.key.sign(msg).to_bytes())
    }

    /// Salt for network hashes: stack-wide from config, else per-device.
    pub fn salt(&self, cfg: &Config) -> Vec<u8> {
        cfg.network
            .salt_hex
            .as_deref()
            .and_then(unhex)
            .filter(|s| s.len() >= 16)
            .or_else(|| unhex(&self.state.salt_hex))
            .unwrap_or_default()
    }

    /// Current network hash (if observable) and whether it is trusted.
    pub fn current_network(
        &self,
        cfg: &Config,
        obs: &wifi::Observation,
    ) -> Option<(String, &'static str, bool)> {
        let (h, kind) = wifi::network_hash(&self.salt(cfg), obs)?;
        let trusted = self.state.trusted_networks.iter().any(|t| t.hash == h);
        Some((h, kind, trusted))
    }

    /// Bind (trust) the current network. The owner runs this only on a
    /// network they control.
    pub fn bind(
        &mut self,
        cfg: &Config,
        obs: &wifi::Observation,
    ) -> Result<(String, &'static str, bool), String> {
        let (h, kind, trusted) = self.current_network(cfg, obs).ok_or(
            "cannot observe this network (no SSID, BSSID or gateway MAC readable); nothing bound",
        )?;
        if !trusted {
            self.state.trusted_networks.push(TrustedNetwork {
                hash: h.clone(),
                anchor: kind.into(),
                bound_at: zagreb::now_rfc3339(),
            });
            self.save()?;
        }
        Ok((h, kind, !trusted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "omnode-test-{tag}-{}",
            hex(&random_bytes::<6>().unwrap())
        ));
        d
    }

    #[test]
    fn init_load_and_permissions() {
        let d = tmpdir("id");
        let a = Identity::init(&d).unwrap();
        let mode = fs::metadata(d.join("node.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700);
        assert!(Identity::init(&d).is_err(), "must not overwrite a key");
        let b = Identity::load(&d).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert!(b.state.device_id.starts_with("omn-"));
        // State file never contains the secret seed.
        let seed = fs::read_to_string(d.join("node.key")).unwrap();
        let st = fs::read_to_string(d.join("node-state.toml")).unwrap();
        assert!(!st.contains(seed.trim()));
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn refuses_loose_key_permissions() {
        let d = tmpdir("perm");
        Identity::init(&d).unwrap();
        fs::set_permissions(d.join("node.key"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Identity::load(&d).is_err());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn sign_verifies() {
        let d = tmpdir("sig");
        let id = Identity::init(&d).unwrap();
        let sig = id.sign(b"hello");
        let sig = ed25519_dalek::Signature::from_slice(&unhex(&sig).unwrap()).unwrap();
        assert!(id.verifying_key().verify(b"hello", &sig).is_ok());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bind_network() {
        let d = tmpdir("bind");
        let mut id = Identity::init(&d).unwrap();
        let cfg = Config::default();
        let obs = wifi::Observation {
            ssid: Some("HomeNet".into()),
            gateway_mac: Some("aa:bb:cc:dd:ee:ff".into()),
            ..Default::default()
        };
        let (h, _, new) = id.bind(&cfg, &obs).unwrap();
        assert!(new);
        let (_, _, again) = id.bind(&cfg, &obs).unwrap();
        assert!(!again);
        let st = fs::read_to_string(d.join("node-state.toml")).unwrap();
        assert!(st.contains(&h));
        assert!(!st.contains("HomeNet"), "raw SSID must never be stored");
        assert!(
            !st.contains("aa:bb:cc:dd:ee:ff"),
            "raw MAC must never be stored"
        );
        assert!(id.bind(&cfg, &wifi::Observation::default()).is_err());
        fs::remove_dir_all(&d).ok();
    }
}
