//! The signed-in token in the OS keychain (Credential Manager on Windows,
//! Keychain on macOS, Secret Service on Linux). Never on disk elsewhere.

const SERVICE: &str = "dev.cloudrs.cloudrs";
const ACCOUNT: &str = "soundcloud-oauth-token";

fn entry() -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT)
}

/// The saved token, if any. A keychain that cannot be read counts as none.
pub fn load_token() -> Option<String> {
    match entry().and_then(|entry| entry.get_password()) {
        Ok(token) => Some(token),
        Err(keyring::Error::NoEntry) => None,
        Err(error) => {
            tracing::warn!(%error, "could not read the keychain");
            None
        }
    }
}

/// Saves the token, replacing any previous one.
pub fn save_token(token: &str) {
    if let Err(error) = entry().and_then(|entry| entry.set_password(token)) {
        tracing::warn!(%error, "could not save the token in the keychain");
    }
}

/// Forgets the token. Nothing saved is not an error.
pub fn delete_token() {
    match entry().and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => tracing::warn!(%error, "could not delete the token from the keychain"),
    }
}
