use keyring::Entry;

use crate::logging;

const SECRET_SERVICE: &str = "dev.zethrus.TerminalTiler";

#[derive(Clone, Debug, Default)]
pub struct SecretStore;

impl SecretStore {
    pub fn new() -> Self {
        Self
    }

    pub fn save_secret(&self, secret_ref: &str, value: &str) -> Result<(), String> {
        if secret_ref.trim().is_empty() {
            return Err("secret reference is empty".into());
        }

        Entry::new(SECRET_SERVICE, secret_ref)
            .map_err(|error| error.to_string())?
            .set_password(value)
            .map_err(|error| error.to_string())
    }

    pub fn load_secret(&self, secret_ref: &str) -> Result<Option<String>, String> {
        if secret_ref.trim().is_empty() {
            return Ok(None);
        }

        let entry = Entry::new(SECRET_SERVICE, secret_ref).map_err(|error| error.to_string())?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn delete_secret(&self, secret_ref: &str) -> Result<(), String> {
        if secret_ref.trim().is_empty() {
            return Ok(());
        }

        let entry = Entry::new(SECRET_SERVICE, secret_ref).map_err(|error| error.to_string())?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
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
