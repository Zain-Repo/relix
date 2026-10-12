//! Remembered passwords use the user's OS credential store, never workspace files.

use crate::database::{ConnectionConfig, Engine};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

pub trait PasswordStore: Send + Sync {
    fn load(&self, config: &ConnectionConfig) -> Result<Option<String>, String>;
    fn save(&self, config: &ConnectionConfig) -> Result<(), String>;
    fn remove(&self, config: &ConnectionConfig) -> Result<(), String>;
}

#[derive(Serialize, Deserialize, PartialEq)]
struct Endpoint {
    engine: Engine,
    host: String,
    port: u16,
    database: String,
    user: String,
    tls: bool,
}

impl From<&ConnectionConfig> for Endpoint {
    fn from(config: &ConnectionConfig) -> Self {
        Self {
            engine: config.engine,
            host: config.host.clone(),
            port: config.port,
            database: config.database.clone(),
            user: config.user.clone(),
            tls: config.tls,
        }
    }
}

pub fn same_endpoint(a: &ConnectionConfig, b: &ConnectionConfig) -> bool {
    Endpoint::from(a) == Endpoint::from(b)
}

#[derive(Serialize, Deserialize)]
struct SecretRecord {
    version: u8,
    endpoint: Endpoint,
    password: String,
}

impl Drop for SecretRecord {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

fn server(config: &ConnectionConfig) -> bool {
    matches!(config.engine, Engine::Postgres | Engine::MySql)
}

pub struct NativePasswordStore;

pub fn native_store() -> Arc<dyn PasswordStore> {
    #[cfg(not(test))]
    {
        Arc::new(NativePasswordStore)
    }
    #[cfg(test)]
    {
        Arc::new(MemoryPasswordStore::default())
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod native {
    use super::*;
    use std::sync::Mutex;
    const SERVICE: &str = "Relix.DatabaseStudio.Passwords.v1";
    // Windows generic credentials cap their blob at 2560 bytes. Use a common
    // bounded envelope across platforms, including endpoint binding metadata.
    const MAX_SECRET_BYTES: usize = 2400;
    static ACCESS: Mutex<()> = Mutex::new(());

    fn entry(config: &ConnectionConfig) -> Result<keyring::Entry, String> {
        if config.id.is_empty() || config.id.len() > 128 || config.id.chars().any(char::is_control)
        {
            return Err("The profile has no valid credential identity.".into());
        }
        keyring::Entry::new(SERVICE, &config.id)
            .map_err(|_| "The OS credential store is unavailable.".into())
    }

    impl PasswordStore for NativePasswordStore {
        fn load(&self, config: &ConnectionConfig) -> Result<Option<String>, String> {
            if !server(config) || !config.remember_password {
                return Ok(None);
            }
            let _guard = ACCESS
                .lock()
                .map_err(|_| "The OS credential store is unavailable.")?;
            let bytes = match entry(config)?.get_secret() {
                Ok(bytes) => Zeroizing::new(bytes),
                Err(keyring::Error::NoEntry) => return Ok(None),
                // Backend errors can contain credential bytes; never format them.
                Err(_) => return Err("Could not read the saved password. Re-enter it or unlock your OS credential store.".into()),
            };
            if bytes.len() > MAX_SECRET_BYTES {
                return Err("The saved credential is invalid. Re-enter the password.".into());
            }
            let mut record: SecretRecord = serde_json::from_slice(&bytes)
                .map_err(|_| "The saved credential is invalid. Re-enter the password.")?;
            if record.version != 1 || record.endpoint != Endpoint::from(config) {
                return Ok(None);
            }
            Ok(Some(std::mem::take(&mut record.password)))
        }

        fn save(&self, config: &ConnectionConfig) -> Result<(), String> {
            if !server(config) || !config.remember_password || config.password.is_empty() {
                return self.remove(config);
            }
            let record = SecretRecord {
                version: 1,
                endpoint: Endpoint::from(config),
                password: config.password.clone(),
            };
            let bytes = Zeroizing::new(
                serde_json::to_vec(&record)
                    .map_err(|_| "Could not prepare the saved credential.")?,
            );
            if bytes.len() > MAX_SECRET_BYTES {
                return Err("Password and connection details exceed the OS credential size limit; this password remains session-only.".into());
            }
            let _guard = ACCESS
                .lock()
                .map_err(|_| "The OS credential store is unavailable.")?;
            entry(config)?.set_secret(&bytes).map_err(|_| "Could not save the password in the OS credential store; this password remains session-only.".into())
        }

        fn remove(&self, config: &ConnectionConfig) -> Result<(), String> {
            let _guard = ACCESS
                .lock()
                .map_err(|_| "The OS credential store is unavailable.")?;
            match entry(config)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => {
                    Err("Could not remove the saved password from the OS credential store.".into())
                }
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl PasswordStore for NativePasswordStore {
    fn load(&self, _: &ConnectionConfig) -> Result<Option<String>, String> {
        Err("Secure credential storage is unsupported on this platform.".into())
    }
    fn save(&self, _: &ConnectionConfig) -> Result<(), String> {
        Err("Secure credential storage is unsupported on this platform.".into())
    }
    fn remove(&self, _: &ConnectionConfig) -> Result<(), String> {
        Err("Secure credential storage is unsupported on this platform.".into())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryPasswordStore(std::sync::Mutex<std::collections::HashMap<String, SecretRecord>>);

#[cfg(test)]
impl PasswordStore for MemoryPasswordStore {
    fn load(&self, config: &ConnectionConfig) -> Result<Option<String>, String> {
        let entries = self
            .0
            .lock()
            .map_err(|_| "Test credential store unavailable")?;
        Ok(entries
            .get(&config.id)
            .filter(|record| config.remember_password && record.endpoint == Endpoint::from(config))
            .map(|record| record.password.clone()))
    }
    fn save(&self, config: &ConnectionConfig) -> Result<(), String> {
        if !server(config) || !config.remember_password || config.password.is_empty() {
            return self.remove(config);
        }
        self.0
            .lock()
            .map_err(|_| "Test credential store unavailable")?
            .insert(
                config.id.clone(),
                SecretRecord {
                    version: 1,
                    endpoint: Endpoint::from(config),
                    password: config.password.clone(),
                },
            );
        Ok(())
    }
    fn remove(&self, config: &ConnectionConfig) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Test credential store unavailable")?
            .remove(&config.id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ConnectionConfig {
        ConnectionConfig {
            id: crate::workspace::new_id("credential-probe"),
            engine: Engine::Postgres,
            database: "synthetic_database".into(),
            user: "synthetic_user".into(),
            password: "synthetic-password-雪'\\".into(),
            ..Default::default()
        }
    }

    #[test]
    fn metadata_and_debug_never_contain_password_and_legacy_profiles_remain_session_only() {
        let config = profile();
        let json = serde_json::to_string(&config).expect("Metadata");
        assert!(!json.contains(&config.password));
        assert!(!format!("{config:?}").contains(&config.password));
        let mut legacy = serde_json::to_value(&config).expect("Metadata");
        legacy
            .as_object_mut()
            .expect("Object")
            .remove("remember_password");
        let decoded: ConnectionConfig = serde_json::from_value(legacy).expect("Legacy profile");
        assert!(!decoded.remember_password);
        assert!(decoded.password.is_empty());
    }

    #[test]
    fn remembered_password_is_bound_to_endpoint_and_opt_out_deletes_it() {
        let store = MemoryPasswordStore::default();
        let config = profile();
        store.save(&config).expect("Save");
        assert_eq!(
            store.load(&config).expect("Restore"),
            Some(config.password.clone())
        );
        for field in 0..6 {
            let mut changed = config.clone();
            match field {
                0 => changed.host = "other.invalid".into(),
                1 => changed.port += 1,
                2 => changed.user = "other".into(),
                3 => changed.database = "other".into(),
                4 => changed.engine = Engine::MySql,
                _ => changed.tls = !changed.tls,
            }
            assert!(store.load(&changed).expect("Different target").is_none());
        }
        let mut renamed = config.clone();
        renamed.name = "New display name".into();
        assert!(store.load(&renamed).expect("Rename").is_some());
        renamed.remember_password = false;
        store.save(&renamed).expect("Forget");
        assert!(store.load(&config).expect("Absent").is_none());
        store.remove(&config).expect("Idempotent remove");
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    #[ignore = "Explicit OS-store probe; creates and removes only a unique synthetic credential"]
    fn native_store_round_trip() {
        let config = profile();
        struct Cleanup(ConnectionConfig);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                // Best-effort cleanup also runs after a failed assertion; the
                // normal test path verifies deletion explicitly below.
                let _ = NativePasswordStore.remove(&self.0);
            }
        }
        let _cleanup = Cleanup(config.clone());
        let store = NativePasswordStore;
        assert!(store.load(&config).expect("Initially absent").is_none());
        store.save(&config).expect("Native write");
        assert_eq!(
            store.load(&config).expect("Native read"),
            Some(config.password.clone())
        );
        let mut changed = config.clone();
        changed.host = "other.invalid".into();
        assert!(store.load(&changed).expect("Target binding").is_none());
        changed = config.clone();
        changed.password = "x".repeat(2500);
        assert!(store.save(&changed).is_err());
        assert_eq!(
            store.load(&config).expect("Previous entry retained"),
            Some(config.password.clone())
        );
        changed = config.clone();
        changed.password.clear();
        store.save(&changed).expect("Empty password removes entry");
        assert!(store.load(&config).expect("Deleted").is_none());
        store.remove(&config).expect("Idempotent removal");
    }
}
