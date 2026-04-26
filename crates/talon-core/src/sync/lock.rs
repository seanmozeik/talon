//! Advisory file lock with stale-PID detection.
//!
//! Ports `services/talon/sync/sync-lock.ts`. The lock is implemented as a
//! `JSON` file containing the holding process's `pid` and start timestamp.
//! Acquisition opens the file with `O_CREAT | O_EXCL`; if creation fails
//! because the file exists, the holder's `pid` is checked with
//! [`rustix::process::test_kill_process`]. If the holder is gone the stale
//! lock is removed and a single retry is attempted.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs_err as fs;
#[cfg(not(windows))]
use rustix::process::{Pid, test_kill_process};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// On-disk representation of the lock file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockMetadata {
    /// Operating-system process id of the holder.
    pub pid: u32,
    /// Wall-clock acquisition time, milliseconds since `UNIX_EPOCH`.
    pub started_at: u64,
}

/// Errors returned by [`acquire_sync_lock`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyncLockError {
    /// Another live process holds the lock.
    #[error("another talon sync is in progress")]
    Busy,
    /// Filesystem operation failed for a reason other than contention.
    #[error("lock io error: {0}")]
    Io(#[from] std::io::Error),
}

/// RAII guard that releases the lock on drop if (and only if) the current
/// process still owns it. Use [`acquire_sync_lock`] to construct one.
#[must_use = "the lock is released as soon as the guard is dropped"]
#[derive(Debug)]
pub struct SyncLock {
    path: PathBuf,
    pid: u32,
}

impl SyncLock {
    /// Returns the path of the lock file backing this guard.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        if let Some(holder) = read_pid(&self.path)
            && holder == self.pid
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Acquires the sync lock at `path`, creating any missing parent directories.
///
/// Performs one stale-lock recovery attempt: if the file exists but its
/// recorded `pid` is no longer alive, it is removed and re-acquired. If the
/// recorded `pid` is alive (or the file cannot be parsed), [`SyncLockError::Busy`]
/// is returned.
///
/// # Errors
///
/// Returns [`SyncLockError::Busy`] under contention and [`SyncLockError::Io`]
/// for any other filesystem failure.
pub fn acquire_sync_lock(path: &Path) -> Result<SyncLock, SyncLockError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if try_create_lock(path)? {
        return Ok(SyncLock {
            path: path.to_path_buf(),
            pid: std::process::id(),
        });
    }

    if try_remove_stale_lock(path)? && try_create_lock(path)? {
        return Ok(SyncLock {
            path: path.to_path_buf(),
            pid: std::process::id(),
        });
    }
    Err(SyncLockError::Busy)
}

/// Returns `true` if the lock file at `path` exists and names a live process.
#[must_use]
pub fn is_sync_lock_held_by_live_process(path: &Path) -> bool {
    let Some(pid) = read_pid(path) else {
        return false;
    };
    is_process_alive(pid)
}

fn try_create_lock(path: &Path) -> Result<bool, SyncLockError> {
    use std::io::Write as _;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            let metadata = LockMetadata {
                pid: std::process::id(),
                started_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            };
            let payload = serde_json::to_string(&metadata).unwrap_or_else(|_| String::from("{}"));
            file.write_all(payload.as_bytes())?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(SyncLockError::Io(err)),
    }
}

fn try_remove_stale_lock(path: &Path) -> Result<bool, SyncLockError> {
    let Some(pid) = read_pid(path) else {
        // Unparseable lock — treat as stale and try removal.
        match fs::remove_file(path) {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(err) => return Err(SyncLockError::Io(err)),
        }
    };
    if is_process_alive(pid) {
        return Ok(false);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(SyncLockError::Io(err)),
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    let metadata: LockMetadata = serde_json::from_str(&raw).ok()?;
    Some(metadata.pid)
}

#[cfg(not(windows))]
fn is_process_alive(pid: u32) -> bool {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return false;
    };
    let Some(typed_pid) = Pid::from_raw(raw_pid) else {
        return false;
    };
    test_kill_process(typed_pid).is_ok()
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "checking Windows process liveness requires Win32 handle APIs"
)]
fn is_process_alive(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }

    let mut exit_code = 0;
    let is_alive = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0
        && exit_code == u32::try_from(STILL_ACTIVE).unwrap_or(259);
    unsafe {
        CloseHandle(handle);
    }
    is_alive
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_lock_path(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        temp_dir()
            .join(format!("talon-lock-test-{label}-{pid}-{n}"))
            .join("sync.lock")
    }

    #[test]
    fn lock_is_acquired_then_released_on_drop() {
        let path = unique_lock_path("rt");
        {
            let lock = acquire_sync_lock(&path).unwrap();
            assert_eq!(lock.path(), path.as_path());
            assert!(path.exists());
        }
        // After drop, the file should be gone.
        assert!(!path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn second_acquisition_while_held_returns_busy() {
        let path = unique_lock_path("busy");
        let _held = acquire_sync_lock(&path).unwrap();
        let result = acquire_sync_lock(&path);
        assert!(matches!(result, Err(SyncLockError::Busy)));
        // _held drops at end of test, releasing the lock.
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stale_lock_with_dead_pid_is_recovered() {
        let path = unique_lock_path("stale");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // Pid 1 is the init process — definitely alive — so we want a guaranteed
        // dead pid. Use the highest possible pid value, which on every common
        // OS is far above any real PID.
        let stale = LockMetadata {
            pid: u32::MAX - 1,
            started_at: 0,
        };
        fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();

        let lock = acquire_sync_lock(&path).unwrap();
        assert!(path.exists());
        drop(lock);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unparseable_lock_is_recovered() {
        let path = unique_lock_path("garbage");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, "not json at all").unwrap();
        let _lock = acquire_sync_lock(&path).unwrap();
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn is_held_returns_false_when_no_lock_file() {
        let path = unique_lock_path("missing");
        assert!(!is_sync_lock_held_by_live_process(&path));
    }
}
