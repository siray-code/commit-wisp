//! API credentials stored outside project configuration.

use anyhow::{Context, Result};

const SERVICE: &str = "commit-wisp";

pub struct SystemSecretStore;

impl SystemSecretStore {
    pub fn set(provider: &str, value: &str) -> Result<()> {
        anyhow::ensure!(!value.trim().is_empty(), "API key cannot be empty");
        keyring::Entry::new(SERVICE, provider)
            .context("Could not open the system credential store")?
            .set_password(value)
            .context("Could not save API key in the system credential store")?;

        let persisted = Self::get_stored(provider)?;
        anyhow::ensure!(
            persisted.as_deref() == Some(value),
            "The system credential store did not persist the API key"
        );
        Ok(())
    }

    pub fn get(provider: &str) -> Result<Option<String>> {
        if let Ok(value) = std::env::var("COMMIT_WISP_API_KEY") {
            if !value.trim().is_empty() {
                return Ok(Some(value));
            }
        }
        Self::get_stored(provider)
    }

    fn get_stored(provider: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .context("Could not open the system credential store")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => {
                Err(error).context("Could not read API key from the system credential store")
            }
        }
    }

    pub fn delete(provider: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .context("Could not open the system credential store")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => {
                Err(error).context("Could not remove API key from the system credential store")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_key_has_priority_without_keychain_access() {
        std::env::set_var("COMMIT_WISP_API_KEY", "test-environment-key");
        assert_eq!(
            SystemSecretStore::get("openai-compatible")
                .unwrap()
                .as_deref(),
            Some("test-environment-key")
        );
        std::env::remove_var("COMMIT_WISP_API_KEY");
    }
}
