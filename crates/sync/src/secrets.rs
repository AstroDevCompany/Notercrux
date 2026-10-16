//! Where device keys and the workspace key ring live on this machine.
//!
//! The desktop app implements [`SecretStore`] on the operating system's secure storage (Keychain,
//! Credential Manager, Secret Service). [`FileSecrets`] is the fallback when that is unavailable.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, name: &str, value: &[u8]) -> Result<(), String>;
    fn delete(&self, name: &str) -> Result<(), String>;
}

/// Keeps secrets in memory only (tests).
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, Vec<u8>>>);

impl MemorySecrets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test helper.
    pub fn get_for_test(&self, name: &str) -> Option<Vec<u8>> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned()
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
            .cloned())
    }

    fn set(&self, name: &str, value: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), value.to_vec());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
        Ok(())
    }
}

/// One file per secret in a directory that only the current user can read. **Weaker than the OS
/// secure storage**: anything running as the same user can read these files. The app tells the user
/// when it has to fall back to this.
pub struct FileSecrets {
    dir: PathBuf,
}

impl FileSecrets {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.secret"))
    }
}

impl SecretStore for FileSecrets {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        match std::fs::read(self.path(name)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, name: &str, value: &[u8]) -> Result<(), String> {
        let path = self.path(name);
        let tmp = path.with_extension("tmp");
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|e| e.to_string())?;
            f.write_all(value).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&tmp, value).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    fn delete(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_secrets_round_trip_and_are_private() {
        let dir =
            std::env::temp_dir().join(format!("ncx-secrets-{}", uuid::Uuid::new_v4().simple()));
        let s = FileSecrets::new(&dir).unwrap();
        assert_eq!(s.get("device").unwrap(), None);
        s.set("device", b"\x01\x02").unwrap();
        s.set("device", b"\x03").unwrap();
        assert_eq!(s.get("device").unwrap(), Some(vec![3]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("device.secret"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o077,
                0,
                "secret files must not be readable by others"
            );
        }
        s.delete("device").unwrap();
        s.delete("device").unwrap();
        assert_eq!(s.get("device").unwrap(), None);
        // Names cannot escape the directory.
        s.set("../evil", b"x").unwrap();
        assert!(dir.join("___evil.secret").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
