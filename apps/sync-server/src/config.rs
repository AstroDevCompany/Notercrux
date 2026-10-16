//! Server configuration. Everything comes from environment variables so a container needs no files.

use notercrux_protocol::Limits;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Registration {
    /// Anyone who can reach the server may create a workspace (private networks, trials).
    Open,
    /// Creating a workspace needs one of these tokens (self-hosters; hosted sign-up gates).
    Tokens(Vec<String>),
    /// No new workspaces.
    Closed,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    /// `None` runs on the in-memory repository.
    pub database_url: Option<String>,
    /// PEM bundle of extra certificate authorities to trust for the database connection.
    pub database_ca: Option<PathBuf>,
    /// `None` keeps attachment chunks in memory.
    pub blob_dir: Option<PathBuf>,
    pub registration: Registration,
    pub limits: Limits,
    pub max_clock_skew: i64,
    pub invite_ttl: i64,
    /// Requests per minute per device.
    pub device_rate: u32,
    /// Requests per minute per client address.
    pub ip_rate: u32,
    pub max_body: usize,
    pub long_poll_max: u64,
    /// When set, `/metrics` needs `Authorization: Bearer <token>`.
    pub metrics_token: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".parse().expect("valid address"),
            database_url: None,
            database_ca: None,
            blob_dir: None,
            registration: Registration::Open,
            limits: Limits {
                max_devices: 10,
                max_op_bytes_total: 2 * 1024 * 1024 * 1024,
                max_blob_bytes_total: 10 * 1024 * 1024 * 1024,
                max_op_size: 4 * 1024 * 1024,
                max_blob_size: 512 * 1024 * 1024,
            },
            max_clock_skew: 300,
            invite_ttl: 900,
            device_rate: 1200,
            ip_rate: 6000,
            max_body: 9 * 1024 * 1024,
            long_poll_max: 25,
            metrics_token: None,
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut c = Self::default();
        let num = |k: &str| -> Result<Option<i64>, String> {
            match get(k) {
                Some(v) => v
                    .trim()
                    .parse::<i64>()
                    .map(Some)
                    .map_err(|_| format!("{k} must be a whole number")),
                None => Ok(None),
            }
        };
        if let Some(v) = get("NCX_BIND") {
            c.bind = v
                .parse()
                .map_err(|_| "NCX_BIND must look like 0.0.0.0:8787".to_string())?;
        }
        c.metrics_token = get("NCX_METRICS_TOKEN").filter(|s| !s.is_empty());
        c.database_ca = get("NCX_DATABASE_CA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        c.database_url = get("NCX_DATABASE_URL").filter(|s| !s.is_empty());
        c.blob_dir = get("NCX_BLOB_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        if let Some(v) = get("NCX_REGISTRATION") {
            c.registration = match v.trim() {
                "open" => Registration::Open,
                "closed" => Registration::Closed,
                t if t.starts_with("token:") => {
                    let tokens: Vec<String> = t["token:".len()..]
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if tokens.is_empty() {
                        return Err("NCX_REGISTRATION=token: needs at least one token".into());
                    }
                    Registration::Tokens(tokens)
                }
                _ => return Err("NCX_REGISTRATION must be open, closed or token:<a>,<b>".into()),
            };
        }
        if let Some(n) = num("NCX_MAX_DEVICES")? {
            c.limits.max_devices = n;
        }
        if let Some(n) = num("NCX_MAX_OP_BYTES")? {
            c.limits.max_op_bytes_total = n;
        }
        if let Some(n) = num("NCX_MAX_BLOB_BYTES")? {
            c.limits.max_blob_bytes_total = n;
        }
        if let Some(n) = num("NCX_MAX_OP_SIZE")? {
            c.limits.max_op_size = n;
        }
        if let Some(n) = num("NCX_MAX_BLOB_SIZE")? {
            c.limits.max_blob_size = n;
        }
        if let Some(n) = num("NCX_DEVICE_RATE")? {
            c.device_rate = n as u32;
        }
        if let Some(n) = num("NCX_IP_RATE")? {
            c.ip_rate = n as u32;
        }
        Ok(c)
    }
}

/// Decides each workspace's quotas. Self-hosted servers use the static limits from the
/// configuration; a hosted deployment can implement this against its billing records.
pub trait Entitlements: Send + Sync {
    fn limits(&self, workspace_id: &str) -> Limits;
}

pub struct StaticEntitlements(pub Limits);

impl Entitlements for StaticEntitlements {
    fn limits(&self, _workspace_id: &str) -> Limits {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_vars(|k| m.get(k).cloned())
    }

    #[test]
    fn defaults_are_conservative_and_local() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.bind.to_string(), "127.0.0.1:8787");
        assert_eq!(c.registration, Registration::Open);
        assert!(c.database_url.is_none());
    }

    #[test]
    fn registration_and_limits_parse() {
        let c = cfg(&[
            ("NCX_REGISTRATION", "token:a, b"),
            ("NCX_MAX_DEVICES", "3"),
            ("NCX_BIND", "0.0.0.0:9000"),
        ])
        .unwrap();
        assert_eq!(
            c.registration,
            Registration::Tokens(vec!["a".into(), "b".into()])
        );
        assert_eq!(c.limits.max_devices, 3);
        assert_eq!(c.bind.port(), 9000);
        assert!(cfg(&[("NCX_REGISTRATION", "token:")]).is_err());
        assert!(cfg(&[("NCX_REGISTRATION", "maybe")]).is_err());
        assert!(cfg(&[("NCX_MAX_DEVICES", "many")]).is_err());
        assert!(cfg(&[("NCX_BIND", "nope")]).is_err());
    }
}
