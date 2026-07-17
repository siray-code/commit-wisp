//! API credentials stored outside non-secret project configuration.

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::global_config_path;

const SERVICE: &str = "commit-wisp";

#[derive(Default, Deserialize, Serialize)]
struct FileCredentials {
    credentials: HashMap<String, String>,
}

pub struct SecretStore;

impl SecretStore {
    pub fn set(store: &str, provider: &str, value: &str) -> Result<()> {
        match store {
            "system" => Self::set_system(provider, value),
            "file" => Self::set_file(&credentials_file_path()?, provider, value),
            _ => anyhow::bail!("Unsupported credential store: {store}"),
        }
    }

    pub fn get(store: &str, provider: &str) -> Result<Option<String>> {
        if let Ok(value) = std::env::var("COMMIT_WISP_API_KEY") {
            if !value.trim().is_empty() {
                return Ok(Some(value));
            }
        }
        match store {
            "system" => Self::get_system(provider),
            "file" => Self::get_file(&credentials_file_path()?, provider),
            _ => anyhow::bail!("Unsupported credential store: {store}"),
        }
    }

    pub fn delete(store: &str, provider: &str) -> Result<()> {
        match store {
            "system" => Self::delete_system(provider),
            "file" => Self::delete_file(&credentials_file_path()?, provider),
            _ => anyhow::bail!("Unsupported credential store: {store}"),
        }
    }

    fn set_system(provider: &str, value: &str) -> Result<()> {
        anyhow::ensure!(!value.trim().is_empty(), "API key cannot be empty");
        keyring::Entry::new(SERVICE, provider)
            .context("Could not open the system credential store")?
            .set_password(value)
            .context("Could not save API key in the system credential store")?;

        let persisted = Self::get_system(provider)?;
        anyhow::ensure!(
            persisted.as_deref() == Some(value),
            "The system credential store did not persist the API key"
        );
        Ok(())
    }

    fn get_system(provider: &str) -> Result<Option<String>> {
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

    fn delete_system(provider: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, provider)
            .context("Could not open the system credential store")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => {
                Err(error).context("Could not remove API key from the system credential store")
            }
        }
    }

    fn set_file(path: &Path, provider: &str, value: &str) -> Result<()> {
        anyhow::ensure!(!value.trim().is_empty(), "API key cannot be empty");
        let mut stored = read_file_credentials(path)?;
        stored.credentials.insert(provider.into(), value.into());
        write_file_credentials(path, &stored)
    }

    fn get_file(path: &Path, provider: &str) -> Result<Option<String>> {
        Ok(read_file_credentials(path)?.credentials.remove(provider))
    }

    fn delete_file(path: &Path, provider: &str) -> Result<()> {
        let mut stored = read_file_credentials(path)?;
        if stored.credentials.remove(provider).is_some() {
            write_file_credentials(path, &stored)?;
        }
        Ok(())
    }
}

pub fn credentials_file_path() -> Result<PathBuf> {
    Ok(global_config_path()?.with_file_name("credentials.toml"))
}

fn read_file_credentials(path: &Path) -> Result<FileCredentials> {
    match fs::read_to_string(path) {
        Ok(raw) => toml::from_str(&raw).context("Invalid credentials file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(FileCredentials::default())
        }
        Err(error) => Err(error).context("Could not read credentials file"),
    }
}

fn write_file_credentials(path: &Path, credentials: &FileCredentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Could not create credentials directory")?;
    }
    let encoded = toml::to_string(credentials).context("Could not serialize credentials")?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("Could not open credentials file")?;
    file.write_all(encoded.as_bytes())
        .context("Could not write credentials file")?;
    file.sync_all().context("Could not sync credentials file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_key_has_priority_without_keychain_access() {
        std::env::set_var("COMMIT_WISP_API_KEY", "test-environment-key");
        assert_eq!(
            SecretStore::get("system", "openai-compatible")
                .unwrap()
                .as_deref(),
            Some("test-environment-key")
        );
        std::env::remove_var("COMMIT_WISP_API_KEY");
    }

    #[test]
    fn file_store_round_trips_deletes_and_uses_private_permissions() {
        let temp = tempfile::tempdir().expect("temp credentials");
        let path = temp.path().join("credentials.toml");
        SecretStore::set_file(&path, "openai-compatible", "test-file-key")
            .expect("store credential");
        assert_eq!(
            SecretStore::get_file(&path, "openai-compatible")
                .expect("read credential")
                .as_deref(),
            Some("test-file-key")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
        }
        SecretStore::delete_file(&path, "openai-compatible").expect("delete credential");
        assert_eq!(
            SecretStore::get_file(&path, "openai-compatible").expect("read deleted credential"),
            None
        );
    }

    #[test]
    fn malformed_credentials_file_is_rejected_without_exposing_contents() {
        let temp = tempfile::tempdir().expect("temp credentials");
        let path = temp.path().join("credentials.toml");
        fs::write(&path, "not valid = [toml").expect("write malformed file");
        let error = SecretStore::get_file(&path, "openai-compatible").expect_err("invalid file");
        assert!(error.to_string().contains("Invalid credentials file"));
        assert!(!error.to_string().contains("not valid"));
    }
}
