//! Device keys and the workspace key ring live in the operating system's secure storage
//! (Keychain, Credential Manager, Secret Service). When that is unavailable, a private file in the
//! app data directory is used instead and the UI says so.

use notercrux_sync::{FileSecrets, SecretStore};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod os {
    use notercrux_sync::SecretStore;

    const SERVICE: &str = "app.notercrux.desktop";

    pub struct Keychain;

    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
    }

    impl SecretStore for Keychain {
        fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
            match entry(name)?.get_secret() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        }

        fn set(&self, name: &str, value: &[u8]) -> Result<(), String> {
            entry(name)?.set_secret(value).map_err(|e| e.to_string())
        }

        fn delete(&self, name: &str) -> Result<(), String> {
            match entry(name)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

/// Chooses the backing store on first use, so apps that never enable sync never touch the keychain.
pub struct AutoSecrets {
    fallback_dir: PathBuf,
    choice: OnceLock<(Arc<dyn SecretStore>, &'static str)>,
}

impl AutoSecrets {
    pub fn new(fallback_dir: PathBuf) -> Self {
        Self {
            fallback_dir,
            choice: OnceLock::new(),
        }
    }

    fn pick(&self) -> &(Arc<dyn SecretStore>, &'static str) {
        self.choice.get_or_init(|| {
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            {
                let os: Arc<dyn SecretStore> = Arc::new(os::Keychain);
                let probe = "probe";
                let works = os.set(probe, b"ok").is_ok()
                    && os.get(probe).ok().flatten().as_deref() == Some(b"ok")
                    && os.delete(probe).is_ok();
                if works {
                    return (os, "os");
                }
            }
            match FileSecrets::new(&self.fallback_dir) {
                Ok(f) => (Arc::new(f) as Arc<dyn SecretStore>, "file"),
                Err(_) => (Arc::new(Unavailable) as Arc<dyn SecretStore>, "none"),
            }
        })
    }

    /// `os`, `file` or `none`, once it is known (after the first use).
    pub fn kind(&self) -> Option<&'static str> {
        self.choice.get().map(|c| c.1)
    }
}

impl SecretStore for AutoSecrets {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        self.pick().0.get(name)
    }
    fn set(&self, name: &str, value: &[u8]) -> Result<(), String> {
        self.pick().0.set(name, value)
    }
    fn delete(&self, name: &str) -> Result<(), String> {
        self.pick().0.delete(name)
    }
}

struct Unavailable;

impl SecretStore for Unavailable {
    fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
        Err("no secure storage is available on this system".into())
    }
    fn set(&self, _: &str, _: &[u8]) -> Result<(), String> {
        Err("no secure storage is available on this system".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
