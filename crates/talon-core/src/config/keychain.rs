//! OS keychain-backed credential storage.
//!
//! One JSON blob stored at service="talon", account="credentials" in the OS
//! keychain. The blob is a `BTreeMap<String, String>` where each key is a
//! credential name.
//!
//! On macOS: uses `security-framework` passwords API (`SecItem`, no UI prompt).
//! On Linux: uses `keyring-core` + linux-keyutils.
//! On Windows: uses `keyring-core` + windows-native-keyring-store.

use std::collections::BTreeMap;

use crate::error::TalonError;

const SERVICE: &str = "talon";
const ACCOUNT: &str = "credentials";

fn config_error(e: impl std::fmt::Display) -> TalonError {
    TalonError::Config {
        message: format!("keychain error: {e}"),
    }
}

// ── macOS ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn read_raw() -> Result<Option<Vec<u8>>, TalonError> {
    use security_framework::passwords::get_generic_password;
    match get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.code() == -25300 => Ok(None), // errSecItemNotFound
        Err(e) => Err(config_error(e)),
    }
}

#[cfg(target_os = "macos")]
fn write_raw(data: &[u8]) -> Result<(), TalonError> {
    use security_framework::passwords::set_generic_password;
    set_generic_password(SERVICE, ACCOUNT, data).map_err(config_error)
}

// ── Linux / Windows (keyring-core) ─────────────────────────────────────────

#[cfg(not(target_os = "macos"))]
fn ensure_store() {
    use keyring_core::set_default_store;
    use std::sync::Arc;
    use std::sync::OnceLock;

    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
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

#[cfg(not(target_os = "macos"))]
fn read_raw() -> Result<Option<Vec<u8>>, TalonError> {
    use keyring_core::Entry;
    use keyring_core::error::Error as KeyringError;
    ensure_store();
    match Entry::new(SERVICE, ACCOUNT)
        .map_err(config_error)?
        .get_secret()
    {
        Ok(bytes) => Ok(Some(bytes)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(config_error(e)),
    }
}

#[cfg(not(target_os = "macos"))]
fn write_raw(data: &[u8]) -> Result<(), TalonError> {
    use keyring_core::Entry;
    ensure_store();
    Entry::new(SERVICE, ACCOUNT)
        .map_err(config_error)?
        .set_secret(data)
        .map_err(config_error)
}

// ── Shared blob logic ───────────────────────────────────────────────────────

fn read_blob() -> Result<BTreeMap<String, String>, TalonError> {
    match read_raw()? {
        None => Ok(BTreeMap::new()),
        Some(bytes) => {
            let s = String::from_utf8(bytes).map_err(config_error)?;
            serde_json::from_str(&s).map_err(config_error)
        }
    }
}

fn write_blob(blob: &BTreeMap<String, String>) -> Result<(), TalonError> {
    let json = serde_json::to_string(blob).map_err(config_error)?;
    write_raw(json.as_bytes())
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
