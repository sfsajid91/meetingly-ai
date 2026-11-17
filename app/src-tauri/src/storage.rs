//! Private, atomic JSON files in the application data directory.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Creates `dir` (and parents) and restricts it to the current user.
pub fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Could not secure {}: {e}", dir.display()))?;
    }
    Ok(())
}

fn private_create(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|e| format!("Could not create {}: {e}", path.display()))
}

/// Writes `bytes` to `path` through a synced temporary file and a rename, so a crash leaves either the old or the
/// new contents, never a partial file.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    ensure_private_dir(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| format!("Invalid file name: {}", path.display()))?;
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = private_create(&tmp)?;
        file.write_all(bytes).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
        file.sync_all().map_err(|e| format!("Could not flush {}: {e}", tmp.display()))?;
        drop(file);
        fs::rename(&tmp, path).map_err(|e| format!("Could not replace {}: {e}", path.display()))?;
        // Persist the rename itself.
        #[cfg(unix)]
        if let Ok(handle) = File::open(dir) {
            let _ = handle.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| format!("Could not serialize {}: {e}", path.display()))?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

/// Reads JSON from `path`. `Ok(None)` means the file does not exist.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| format!("{} is not valid: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

/// Moves an unreadable file aside so it can be recovered by hand instead of being overwritten.
pub fn quarantine(path: &Path) -> Result<PathBuf, String> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let target = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    fs::rename(path, &target).map_err(|e| format!("Could not move aside {}: {e}", path.display()))?;
    Ok(target)
}

#[cfg(test)]
pub mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A fresh directory under the system temp directory, removed on drop.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("meetingly-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempDir;
    use super::*;

    #[test]
    fn atomic_write_replaces_contents_and_leaves_no_temp_file() {
        let dir = TempDir::new();
        let file = dir.path().join("nested").join("data.json");
        atomic_write(&file, b"one").unwrap();
        atomic_write(&file, b"two").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap()).unwrap().filter_map(Result::ok).collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn written_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let file = dir.path().join("secret-ish.json");
        write_json(&file, &serde_json::json!({ "a": 1 })).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn quarantine_keeps_the_unreadable_file() {
        let dir = TempDir::new();
        let file = dir.path().join("settings.json");
        fs::write(&file, b"{ not json").unwrap();
        assert!(read_json::<serde_json::Value>(&file).is_err());
        let moved = quarantine(&file).unwrap();
        assert!(!file.exists());
        assert_eq!(fs::read(moved).unwrap(), b"{ not json");
    }
}
