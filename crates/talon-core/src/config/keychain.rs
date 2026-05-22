//! OS keychain-backed credential storage.
//!
//! One JSON blob stored at service="talon", account="credentials" in the OS
//! keychain (macOS Keychain, Linux kernel keyring, Windows Credential Manager).
//! The blob is a `BTreeMap<String, String>` where each key is a credential name.

use std::collections::BTreeMap;
use std::sync::Arc;

use keyring_core::error::Error as KeyringError;
use keyring_core::{Entry, set_default_store};

use crate::error::TalonError;

const SERVICE: &str = "talon";
const ACCOUNT: &str = "credentials";

/// Initializes the platform credential store exactly once.
fn ensure_store() {
    use std::sync::OnceLock;
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            use apple_native_keyring_store::keychain::Store;
            if let Ok(store) = Store::new() {
                set_default_store(store as Arc<_>);
            }
        }
        #[cfg(target_os = "linux")]
        {
            use linux_keyutils_keyring_store::Store;
            if let Ok(store) = Store::new() {
                set_default_store(store as Arc<_>);
            }
        }
        #[cfg(target_os = "windows")]
        {
            use windows_native_keyring_store::Store;
            if let Ok(store) = Store::new() {
                set_default_store(store as Arc<_>);
            }
        }
    });
}

fn entry() -> Result<Entry, TalonError> {
    ensure_store();
    Entry::new(SERVICE, ACCOUNT).map_err(config_error)
}

fn read_blob() -> Result<BTreeMap<String, String>, TalonError> {
    match entry()?.get_password() {
        Ok(raw) => serde_json::from_str(&raw).map_err(config_error),
        Err(KeyringError::NoEntry) => Ok(BTreeMap::new()),
        Err(e) => Err(config_error(e)),
    }
}

fn write_blob(blob: &BTreeMap<String, String>) -> Result<(), TalonError> {
    let json = serde_json::to_string(blob).map_err(config_error)?;
    entry()?.set_password(&json).map_err(config_error)
}

fn config_error(e: impl std::fmt::Display) -> TalonError {
    TalonError::Config {
        message: format!("keychain error: {e}"),
    }
}

/// Returns the API key for `name` from the keychain blob, or `None` if not set.
///
/// # Errors
///
/// Returns [`TalonError::Config`] on keychain or parse failure.
pub fn get(name: &str) -> Result<Option<String>, TalonError> {
    Ok(read_blob()?.remove(name))
}

/// Stores `key` under `name` in the keychain blob.
///
/// # Errors
///
/// Returns [`TalonError::Config`] on keychain or serialize failure.
pub fn set(name: &str, key: &str) -> Result<(), TalonError> {
    let mut blob = read_blob()?;
    blob.insert(name.to_owned(), key.to_owned());
    write_blob(&blob)
}

/// Removes `name` from the keychain blob. Returns `true` if it existed.
///
/// # Errors
///
/// Returns [`TalonError::Config`] on keychain or parse failure.
pub fn delete(name: &str) -> Result<bool, TalonError> {
    let mut blob = read_blob()?;
    let existed = blob.remove(name).is_some();
    if existed {
        write_blob(&blob)?;
    }
    Ok(existed)
}

/// Returns all credential names in the blob (keys only, no values).
///
/// # Errors
///
/// Returns [`TalonError::Config`] on keychain or parse failure.
pub fn list_names() -> Result<Vec<String>, TalonError> {
    Ok(read_blob()?.into_keys().collect())
}
