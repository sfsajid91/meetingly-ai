//! Provider API keys live in the OS keychain, never in settings files or the webview.

/// Keychain service name, distinct from the Electron build's.
pub const SERVICE: &str = "com.meetingly.byok";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    /// No secure keychain is reachable (locked, not running, or unsupported). Nothing was stored.
    Unavailable(String),
    /// The keychain answered but refused or failed the operation.
    Failed(String),
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecretError::Unavailable(detail) => write!(
                f,
                "The system keychain is unavailable, so the API key was not saved. Unlock or start your keychain (for example GNOME Keyring or KWallet) and try again. ({detail})"
            ),
            SecretError::Failed(detail) => write!(f, "The system keychain could not complete the request: {detail}"),
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    /// `Ok(None)` when no secret is stored for `account`.
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    /// Deleting a missing secret succeeds.
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

/// The platform keychain via the `keyring` crate: Secret Service on Linux, Keychain on macOS, Credential Manager on
/// Windows. The crate has no plaintext fallback; an unavailable store is an error.
pub struct OsKeychain;

impl OsKeychain {
    fn entry(account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, account).map_err(map_error)
    }
}

fn map_error(error: keyring::Error) -> SecretError {
    match error {
        keyring::Error::NoDefaultStore | keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            let detail = match keyring::Entry::store_status() {
                Err(status) => status.to_string(),
                Ok(()) => error.to_string(),
            };
            SecretError::Unavailable(detail)
        }
        other => SecretError::Failed(other.to_string()),
    }
}

impl SecretStore for OsKeychain {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        Self::entry(account)?.set_password(secret).map_err(map_error)
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match Self::entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

/// In-process store for tests. `unavailable` simulates a missing keychain.
#[cfg(test)]
#[derive(Default)]
pub struct MemorySecrets {
    pub secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
    pub unavailable: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemorySecrets {
    fn check(&self) -> Result<(), SecretError> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            Err(SecretError::Unavailable("test keychain disabled".into()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl SecretStore for MemorySecrets {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.check()?;
        self.secrets.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self.secrets.lock().unwrap().get(account).cloned())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.check()?;
        self.secrets.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real keychain; run with `cargo test -- --ignored os_keychain` on a desktop session.
    #[test]
    #[ignore]
    fn os_keychain_round_trip() {
        let account = format!("test:{}", std::process::id());
        let store = OsKeychain;
        store.set(&account, "probe-value").unwrap();
        assert_eq!(store.get(&account).unwrap().as_deref(), Some("probe-value"));
        store.delete(&account).unwrap();
        assert_eq!(store.get(&account).unwrap(), None);
        store.delete(&account).unwrap();
    }

    #[test]
    fn unavailable_message_does_not_suggest_plaintext() {
        let message = SecretError::Unavailable("x".into()).to_string();
        assert!(message.contains("was not saved"));
    }
}
