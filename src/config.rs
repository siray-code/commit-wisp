//! Layered configuration without plaintext credentials.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Fully resolved runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub provider: String,
    pub credential_store: String,
    pub model: String,
    pub base_url: String,
    pub language: String,
    pub format: String,
    pub max_input_tokens: usize,
    pub candidates: usize,
    pub timeout_seconds: u64,
    pub prompt_file: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "openai-compatible".into(),
            credential_store: "system".into(),
            model: "gpt-4.1-mini".into(),
            base_url: "https://api.openai.com/v1".into(),
            language: "en".into(),
            format: "conventional".into(),
            max_input_tokens: 12_000,
            candidates: 1,
            timeout_seconds: 30,
            prompt_file: None,
        }
    }
}

/// Inputs used by [`Config::resolve`], exposed for deterministic testing.
pub struct ConfigSources<'a> {
    pub global_toml: Option<&'a str>,
    pub project_toml: Option<&'a str>,
    pub env: &'a HashMap<String, String>,
    pub cli_model: Option<&'a str>,
    pub cli_provider: Option<&'a str>,
}

#[derive(Debug, Default)]
pub struct CliOverrides<'a> {
    pub model: Option<&'a str>,
    pub provider: Option<&'a str>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialConfig {
    provider: Option<String>,
    credential_store: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    format: Option<String>,
    max_input_tokens: Option<usize>,
    candidates: Option<usize>,
    timeout_seconds: Option<u64>,
    prompt_file: Option<String>,
}

impl Config {
    pub fn load(cwd: &Path, overrides: CliOverrides<'_>) -> Result<Self> {
        let global_path = global_config_path()?;
        let global = fs::read_to_string(&global_path).ok();
        let project = find_project_config(cwd).and_then(|path| fs::read_to_string(path).ok());
        let env: HashMap<String, String> = std::env::vars().collect();
        Self::resolve(ConfigSources {
            global_toml: global.as_deref(),
            project_toml: project.as_deref(),
            env: &env,
            cli_model: overrides.model,
            cli_provider: overrides.provider,
        })
    }

    pub fn save_global(&self) -> Result<PathBuf> {
        let path = global_config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Could not create configuration directory")?;
        }
        let encoded = toml::to_string_pretty(self).context("Could not serialize configuration")?;
        fs::write(&path, encoded).context("Could not write global configuration")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(path)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "provider" => self.provider = value.into(),
            "credential_store" => self.credential_store = value.into(),
            "model" => self.model = value.into(),
            "base_url" => self.base_url = value.into(),
            "language" => self.language = value.into(),
            "format" => self.format = value.into(),
            "max_input_tokens" => {
                self.max_input_tokens = value
                    .parse()
                    .context("max_input_tokens must be an integer")?
            }
            "candidates" => {
                self.candidates = value.parse().context("candidates must be an integer")?
            }
            "timeout_seconds" => {
                self.timeout_seconds = value
                    .parse()
                    .context("timeout_seconds must be an integer")?
            }
            "prompt_file" => {
                self.prompt_file = if value.is_empty() {
                    None
                } else {
                    Some(value.into())
                }
            }
            _ => anyhow::bail!("Unknown configuration key: {key}"),
        }
        self.validate()
    }

    /// Resolves defaults, global file, project file, environment, then CLI.
    pub fn resolve(sources: ConfigSources<'_>) -> Result<Self> {
        let mut config = Self::default();
        if let Some(raw) = sources.global_toml {
            config.apply(toml::from_str(raw).context("Invalid global configuration")?);
        }
        if let Some(raw) = sources.project_toml {
            config.apply(toml::from_str(raw).context("Invalid project configuration")?);
        }
        config.apply_env(sources.env)?;
        if let Some(provider) = sources.cli_provider {
            config.provider = provider.into();
        }
        if let Some(model) = sources.cli_model {
            config.model = model.into();
        }
        config.validate()?;
        Ok(config)
    }

    fn apply(&mut self, partial: PartialConfig) {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = partial.$field {
                    self.$field = value;
                }
            };
        }
        apply!(provider);
        apply!(credential_store);
        apply!(model);
        apply!(base_url);
        apply!(language);
        apply!(format);
        apply!(max_input_tokens);
        apply!(candidates);
        apply!(timeout_seconds);
        if let Some(value) = partial.prompt_file {
            self.prompt_file = Some(value);
        }
    }

    fn apply_env(&mut self, env: &HashMap<String, String>) -> Result<()> {
        if let Some(value) = env.get("COMMIT_WISP_PROVIDER") {
            self.provider.clone_from(value);
        }
        if let Some(value) = env.get("COMMIT_WISP_CREDENTIAL_STORE") {
            self.credential_store.clone_from(value);
        }
        if let Some(value) = env.get("COMMIT_WISP_MODEL") {
            self.model.clone_from(value);
        }
        if let Some(value) = env.get("COMMIT_WISP_BASE_URL") {
            self.base_url.clone_from(value);
        }
        if let Some(value) = env.get("COMMIT_WISP_LANGUAGE") {
            self.language.clone_from(value);
        }
        if let Some(value) = env.get("COMMIT_WISP_MAX_INPUT_TOKENS") {
            self.max_input_tokens = value
                .parse()
                .context("COMMIT_WISP_MAX_INPUT_TOKENS must be an integer")?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            matches!(self.provider.as_str(), "openai-compatible" | "ollama"),
            "provider must be openai-compatible or ollama"
        );
        anyhow::ensure!(
            matches!(self.credential_store.as_str(), "system" | "file"),
            "credential_store must be system or file"
        );
        anyhow::ensure!(!self.model.trim().is_empty(), "model cannot be empty");
        anyhow::ensure!(
            self.base_url.starts_with("http://") || self.base_url.starts_with("https://"),
            "base_url must use http or https"
        );
        anyhow::ensure!(
            (256..=1_000_000).contains(&self.max_input_tokens),
            "max_input_tokens must be between 256 and 1000000"
        );
        anyhow::ensure!(
            (1..=10).contains(&self.candidates),
            "candidates must be between 1 and 10"
        );
        Ok(())
    }
}

pub fn global_config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "commit-wisp", "commit-wisp")
        .context("Could not determine user configuration directory")?;
    Ok(dirs.config_dir().join("config.toml"))
}

fn find_project_config(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|path| path.join(".commit-wisp.toml"))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_supports_every_public_key_and_rejects_bad_values() {
        let mut config = Config::default();
        for (key, value) in [
            ("provider", "ollama"),
            ("credential_store", "file"),
            ("model", "qwen3"),
            ("base_url", "http://localhost:11434"),
            ("language", "zh"),
            ("format", "plain"),
            ("max_input_tokens", "4096"),
            ("candidates", "2"),
            ("timeout_seconds", "15"),
            ("prompt_file", "prompt.txt"),
        ] {
            config.set(key, value).expect("valid setting");
        }
        assert_eq!(config.prompt_file.as_deref(), Some("prompt.txt"));
        config.set("prompt_file", "").expect("clear prompt");
        assert_eq!(config.prompt_file, None);
        assert!(config.set("candidates", "99").is_err());
        assert!(config.set("unknown", "value").is_err());
    }
}
