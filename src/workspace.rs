//! Local workbench persistence. Credentials are excluded by ConnectionConfig.
//!
//! Saves use a lock, optimistic revision check, and same-directory replacement
//! so concurrent windows cannot silently overwrite one another's workspace.

use std::{
    collections::HashSet,
    env, fmt, fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::database::ConnectionConfig;

pub const MAX_HISTORY: usize = 100;
pub const MAX_SAVED_QUERIES: usize = 256;
const MAX_WORKSPACE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SQL_BYTES: usize = 512 * 1024;
const VERSION: u32 = 1;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn new_id(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{timestamp:x}-{sequence:x}")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceData {
    pub version: u32,
    pub connections: Vec<ConnectionConfig>,
    pub queries: Vec<SavedQuery>,
    pub history: Vec<HistoryRecord>,
    pub preferences: Preferences,
}

impl Default for WorkspaceData {
    fn default() -> Self {
        Self {
            version: VERSION,
            connections: Vec::new(),
            queries: Vec::new(),
            history: Vec::new(),
            preferences: Preferences::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: String,
    pub title: String,
    pub sql: String,
    pub connection_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub sql: String,
    pub elapsed_ms: u64,
    pub row_count: usize,
    pub succeeded: bool,
    pub message: String,
    pub at_unix_seconds: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub dark_mode: bool,
    pub row_limit: usize,
    pub show_sidebar: bool,
    pub show_results: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            dark_mode: false,
            row_limit: 1000,
            show_sidebar: true,
            show_results: true,
        }
    }
}

#[derive(Debug)]
pub struct StoreError(String);

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

#[derive(Clone)]
pub struct WorkspaceStore {
    path: Option<PathBuf>,
    revision: Arc<Mutex<Option<u64>>>,
}

impl WorkspaceStore {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            revision: Arc::new(Mutex::new(None)),
        }
    }

    pub fn open_default() -> Result<(Self, WorkspaceData), StoreError> {
        Self::open(default_path()?)
    }

    fn open(path: PathBuf) -> Result<(Self, WorkspaceData), StoreError> {
        let bytes = read_bounded(&path)?;
        let revision = bytes.as_deref().map(fingerprint);
        let recovered = if bytes.is_none() {
            read_bounded(&backup_path(&path))?
        } else {
            None
        };
        let data = match bytes.or(recovered) {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| StoreError("The saved workspace is invalid. Preserve workspace.json and its .bak before recovering it.".into()))?,
            None => WorkspaceData::default(),
        };
        validate(&data)?;
        Ok((
            Self {
                path: Some(path),
                revision: Arc::new(Mutex::new(revision)),
            },
            data,
        ))
    }

    pub fn save(&self, data: &WorkspaceData) -> Result<(), StoreError> {
        validate(data)?;
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|_| StoreError("The workspace could not be serialized.".into()))?;
        if bytes.len() as u64 > MAX_WORKSPACE_BYTES {
            return Err(StoreError("The workspace exceeds the 16 MiB storage limit. Remove older saved queries or history entries.".into()));
        }
        let mut revision = self.revision.lock().map_err(|_| {
            StoreError("Workspace storage is unavailable after an earlier failure.".into())
        })?;
        let parent = path
            .parent()
            .ok_or_else(|| StoreError("The workspace location has no parent directory.".into()))?;
        fs::create_dir_all(parent).map_err(|_| {
            StoreError("Cannot create the local workspace directory. Check its permissions.".into())
        })?;
        let lock_path = path.with_extension("json.lock");
        let lock_file = private_file(&lock_path).map_err(|_| StoreError("Another Relix instance is saving this workspace, or a stale workspace.json.lock needs recovery. Changes remain in this window.".into()))?;
        let _lock = SaveLock {
            path: lock_path,
            file: Some(lock_file),
        };
        let current = read_bounded(path)?;
        if current.as_deref().map(fingerprint) != *revision {
            return Err(StoreError("The workspace was changed by another Relix instance. Changes remain in this window; reopen the app before saving again.".into()));
        }
        if let Some(previous) = current {
            replace_file(&backup_path(path), &previous)?;
        }
        replace_file(path, &bytes)?;
        *revision = Some(fingerprint(&bytes));
        Ok(())
    }
}

fn default_path() -> Result<PathBuf, StoreError> {
    #[cfg(target_os = "windows")]
    let root = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root =
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    root.map(|root| root.join("Relix").join("workspace.json"))
        .ok_or_else(|| {
            StoreError(
                "The operating system did not provide a local application data directory.".into(),
            )
        })
}

fn validate(data: &WorkspaceData) -> Result<(), StoreError> {
    if data.version != VERSION {
        return Err(StoreError(
            "This workspace uses an unsupported version. Open it with the matching Relix version."
                .into(),
        ));
    }
    if data.connections.len() > 100
        || data.queries.len() > MAX_SAVED_QUERIES
        || data.history.len() > MAX_HISTORY
    {
        return Err(StoreError(
            "The workspace exceeds its profile, saved query or history limit.".into(),
        ));
    }
    if data
        .queries
        .iter()
        .any(|query| query.sql.len() > MAX_SQL_BYTES)
        || data
            .history
            .iter()
            .any(|entry| entry.sql.len() > MAX_SQL_BYTES)
    {
        return Err(StoreError(
            "A saved query exceeds the 512 KiB SQL limit.".into(),
        ));
    }
    let mut ids = HashSet::new();
    if data
        .connections
        .iter()
        .any(|profile| profile.id.is_empty() || !ids.insert(&profile.id))
    {
        return Err(StoreError(
            "Connection profiles require unique nonempty identifiers.".into(),
        ));
    }
    if !(1..=10_000).contains(&data.preferences.row_limit) {
        return Err(StoreError(
            "The stored row limit must be between 1 and 10,000.".into(),
        ));
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(StoreError(
                "Cannot read the local workspace. Check file permissions.".into(),
            ));
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_WORKSPACE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StoreError("The workspace could not be read completely.".into()))?;
    if bytes.len() as u64 > MAX_WORKSPACE_BYTES {
        return Err(StoreError(
            "The saved workspace exceeds the 16 MiB limit.".into(),
        ));
    }
    Ok(Some(bytes))
}

fn private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let temporary = path.with_extension(format!("{}.tmp", new_id("save")));
    let result = (|| -> std::io::Result<()> {
        let mut file = private_file(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        // Only this save's uniquely named temporary file is eligible for cleanup.
        let _ = fs::remove_file(&temporary);
        return Err(StoreError("Cannot save the workspace. The previous file is preserved; check permissions and available disk space.".into()));
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

struct SaveLock {
    path: PathBuf,
    file: Option<fs::File>,
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        // Close before removal on Windows. Failure preserves a recoverable lock.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(new_id("relix-workspace-test"));
            fs::create_dir(&path).expect("temporary test directory");
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("workspace.json")
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saves_restore_queries_and_exclude_passwords() {
        let directory = TestDirectory::new();
        let (store, mut data) =
            WorkspaceStore::open(directory.path()).expect("open empty workspace");
        data.connections.push(ConnectionConfig {
            id: "connection-1".into(),
            password: "test-only-secret".into(),
            ..Default::default()
        });
        data.queries.push(SavedQuery {
            id: "query-1".into(),
            title: "Totals".into(),
            sql: "SELECT 42;".into(),
            connection_id: Some("connection-1".into()),
        });
        store.save(&data).expect("save workspace");
        let bytes = fs::read_to_string(directory.path()).expect("saved file");
        assert!(!bytes.contains("test-only-secret"));
        let (_, loaded) = WorkspaceStore::open(directory.path()).expect("reload workspace");
        assert_eq!(loaded.queries[0].sql, "SELECT 42;");
        assert!(loaded.connections[0].password.is_empty());
        data.queries[0].sql = "SELECT 43;".into();
        store
            .save(&data)
            .expect("replace existing workspace atomically");
        assert!(backup_path(&directory.path()).exists());
    }

    #[test]
    fn concurrent_windows_cannot_overwrite_and_invalid_files_are_preserved() {
        let directory = TestDirectory::new();
        let (first, data) = WorkspaceStore::open(directory.path()).expect("first window");
        let (second, _) = WorkspaceStore::open(directory.path()).expect("second window");
        first.save(&data).expect("first save");
        assert!(second.save(&data).is_err());
        fs::write(directory.path(), b"invalid workspace").expect("malformed fixture");
        assert!(WorkspaceStore::open(directory.path()).is_err());
        assert_eq!(
            fs::read(directory.path()).expect("preserved fixture"),
            b"invalid workspace"
        );
    }

    #[test]
    fn missing_primary_can_recover_backup_and_future_versions_are_rejected() {
        let directory = TestDirectory::new();
        let (store, mut data) = WorkspaceStore::open(directory.path()).expect("open workspace");
        store.save(&data).expect("first save");
        data.preferences.dark_mode = true;
        store.save(&data).expect("second save");
        fs::remove_file(directory.path()).expect("simulate missing primary");
        let (recovered, old) = WorkspaceStore::open(directory.path()).expect("recover backup");
        assert!(!old.preferences.dark_mode);
        recovered.save(&old).expect("restore primary");
        let mut future = old;
        future.version = VERSION + 1;
        assert!(recovered.save(&future).is_err());
    }
}
