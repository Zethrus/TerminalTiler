use keyring::Entry;
use std::error::Error;
use std::fmt;

use crate::logging;

#[allow(dead_code)]
const SECRET_SERVICE: &str = "dev.zethrus.TerminalTiler";

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct SecretStore;

#[derive(Debug)]
pub enum SecretStoreError {
    EmptySecretReference,
    Keyring(keyring::Error),
}

impl fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySecretReference => formatter.write_str("secret reference is empty"),
            Self::Keyring(error) => error.fmt(formatter),
        }
    }
}

impl Error for SecretStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::EmptySecretReference => None,
            Self::Keyring(error) => Some(error),
        }
    }
}

impl From<keyring::Error> for SecretStoreError {
    fn from(error: keyring::Error) -> Self {
        Self::Keyring(error)
    }
}

#[allow(dead_code)]
impl SecretStore {
    pub fn new() -> Self {
        Self
    }

    pub fn save_secret(&self, secret_ref: &str, value: &str) -> Result<(), SecretStoreError> {
        if secret_ref.trim().is_empty() {
            return Err(SecretStoreError::EmptySecretReference);
        }

        Entry::new(SECRET_SERVICE, secret_ref)
            .map_err(SecretStoreError::from)?
            .set_password(value)
            .map_err(SecretStoreError::from)
    }

    pub fn load_secret(&self, secret_ref: &str) -> Result<Option<String>, SecretStoreError> {
        if secret_ref.trim().is_empty() {
            return Ok(None);
        }

        let entry = Entry::new(SECRET_SERVICE, secret_ref).map_err(SecretStoreError::from)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretStoreError::from(error)),
        }
    }

    pub fn delete_secret(&self, secret_ref: &str) -> Result<(), SecretStoreError> {
        if secret_ref.trim().is_empty() {
            return Ok(());
        }

        let entry = Entry::new(SECRET_SERVICE, secret_ref).map_err(SecretStoreError::from)?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretStoreError::from(error)),
        }
    }

    pub fn is_available(&self) -> bool {
        let probe_key = "__terminaltiler_probe__";
        let probe_value = "probe";
        let result = self
            .save_secret(probe_key, probe_value)
            .and_then(|_| self.delete_secret(probe_key));
        if let Err(error) = result {
            logging::info(format!("secret store unavailable: {error}"));
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{SecretStore, SecretStoreError};

    #[test]
    fn rejects_empty_secret_references_for_save() {
        let store = SecretStore::new();

        let error = store
            .save_secret("   ", "value")
            .expect_err("empty secret references should fail");

        assert!(matches!(error, SecretStoreError::EmptySecretReference));
    }
}
