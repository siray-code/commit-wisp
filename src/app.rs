//! Application orchestration.

use std::{
    fs,
    io::{self, IsTerminal, Write},
};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

use crate::{
    cli::{Cli, Commands, CompletionShell, ConfigAction, SetupArgs},
    compress::{compress_diff, CompressionOptions},
    config::{CliOverrides, Config},
    git::GitRepo,
    prompt::{render_prompt, PromptContext},
    provider::{validate_endpoint, LlmProvider, OllamaProvider, OpenAiProvider},
    secret::SystemSecretStore,
    security::scan_sensitive,
    tui::{self, ReviewAction, ReviewInput},
};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Commands::Setup(args)) => setup(args).await,
        Some(Commands::Config { action }) => config_command(action),
        Some(Commands::Doctor) => doctor(&cli).await,
        Some(Commands::Completions { shell }) => {
            completions(*shell);
            Ok(())
        }
        None => generate_and_review(&cli).await,
    }
}

async fn generate_and_review(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut config = Config::load(
        &cwd,
        CliOverrides {
            model: cli.model.as_deref(),
            provider: cli.provider.as_deref(),
        },
    )?;
    let repo = GitRepo::discover(&cwd)?;
    let diff = repo.staged_diff()?;
    let findings = scan_sensitive(&diff);
    if !findings.is_empty() && !cli.allow_sensitive {
        let locations = findings
            .iter()
            .map(|finding| format!("{}:{} ({})", finding.file, finding.line, finding.rule))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("Sensitive content detected; nothing was sent: {locations}. Inspect the staged diff or explicitly use --allow-sensitive.");
    }
    let compression = compress_diff(
        &diff,
        &CompressionOptions {
            max_tokens: config.max_input_tokens.saturating_sub(1_200),
            ..CompressionOptions::default()
        },
    );
    let stats = repo.diff_stats()?;
    let recent = repo.recent_commits(8)?;
    let custom_template = config
        .prompt_file
        .as_deref()
        .map(fs::read_to_string)
        .transpose()
        .context("Could not read prompt_file")?;
    let prompt = render_prompt(&PromptContext {
        diff: &compression.content,
        stats: &stats,
        recent_commits: &recent,
        language: &config.language,
        format: &config.format,
        extra_instruction: cli.prompt.as_deref(),
        custom_template: custom_template.as_deref(),
    })?;

    loop {
        let provider = create_provider(&config)?;
        let models = provider
            .models()
            .await
            .unwrap_or_else(|_| vec![config.model.clone()]);
        let mut candidates = provider.generate(&prompt, config.candidates).await?;
        if cli.dry_run || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            println!(
                "Staged diff tokens: {} -> {} ({} lines omitted)",
                compression.original_tokens,
                compression.estimated_tokens,
                compression.omitted_lines
            );
            for (index, candidate) in candidates.iter().enumerate() {
                println!("\n{}. {}", index + 1, candidate.message());
            }
            return Ok(());
        }
        match tui::review(ReviewInput {
            candidates: &mut candidates,
            stats: &stats,
            compression: &compression,
            provider: &config.provider,
            model: &config.model,
            models: &models,
        })? {
            ReviewAction::Commit(message) => {
                repo.commit(&message, cli.no_verify)?;
                println!("Committed: {}", message.lines().next().unwrap_or_default());
                return Ok(());
            }
            ReviewAction::Regenerate => {}
            ReviewAction::ChangeModel(model) => config.model = model,
            ReviewAction::Cancel => {
                println!("Cancelled; staged changes were not modified.");
                return Ok(());
            }
        }
    }
}

fn create_provider(config: &Config) -> Result<Box<dyn LlmProvider>> {
    match config.provider.as_str() {
        "ollama" => Ok(Box::new(OllamaProvider::new(
            config.base_url.clone(),
            config.model.clone(),
            config.timeout_seconds,
        )?)),
        "openai-compatible" => {
            let key = SystemSecretStore::get(&config.provider)?;
            anyhow::ensure!(
                key.is_some(),
                "No API key configured. Run commit-wisp setup or set COMMIT_WISP_API_KEY."
            );
            Ok(Box::new(OpenAiProvider::new(
                config.base_url.clone(),
                config.model.clone(),
                key,
                config.timeout_seconds,
            )?))
        }
        _ => anyhow::bail!("Unsupported provider: {}", config.provider),
    }
}

async fn setup(args: &SetupArgs) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut config = Config::load(&cwd, CliOverrides::default())?;
    let previous_provider = config.provider.clone();
    let provider = args.provider.clone().unwrap_or(prompt_value(
        "Provider (openai-compatible/ollama)",
        &config.provider,
    )?);
    config.set("provider", &provider)?;

    let default_url = if config.provider == previous_provider {
        config.base_url.clone()
    } else {
        default_base_url(&config.provider)?.into()
    };
    let base_url = args
        .base_url
        .clone()
        .unwrap_or(prompt_value("Base URL", &default_url)?);
    config.set("base_url", &base_url)?;
    validate_endpoint(&config.base_url)?;

    // Keep confirmed non-secret values even if credential setup fails later.
    let path = config.save_global()?;

    if config.provider == "openai-compatible" {
        let has_existing_key = SystemSecretStore::get(&config.provider)?.is_some();
        if let Some(key) = prompt_api_key(has_existing_key)? {
            SystemSecretStore::set(&config.provider, &key)?;
        }
    }
    let provider = create_provider(&config)?;
    match provider.models().await {
        Ok(models) if !models.is_empty() => {
            println!(
                "Available models (first 12): {}",
                models
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("warning: Could not list provider models: {error:#}");
        }
    }
    let model = args
        .model
        .clone()
        .unwrap_or(prompt_value("Model", &config.model)?);
    config.set("model", &model)?;
    config.save_global()?;
    println!("Saved non-secret configuration to {}", path.display());
    Ok(())
}

fn default_base_url(provider: &str) -> Result<&'static str> {
    match provider {
        "openai-compatible" => Ok("https://api.openai.com/v1"),
        "ollama" => Ok("http://localhost:11434"),
        _ => anyhow::bail!("Unsupported provider: {provider}"),
    }
}

fn prompt_api_key(has_existing_key: bool) -> Result<Option<String>> {
    if !io::stdin().is_terminal() {
        anyhow::ensure!(
            has_existing_key,
            "No API key configured. Run commit-wisp setup in an interactive terminal or set COMMIT_WISP_API_KEY."
        );
        return Ok(None);
    }

    loop {
        let prompt = if has_existing_key {
            "API key (hidden; press Enter to keep the existing credential): "
        } else {
            "API key (hidden; stored in system credential store): "
        };
        let key = rpassword::prompt_password(prompt)?;
        match resolve_api_key_input(&key, has_existing_key) {
            Ok(action) => return Ok(action),
            Err(error) => eprintln!("{error}"),
        }
    }
}

fn resolve_api_key_input(value: &str, has_existing_key: bool) -> Result<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::ensure!(
            has_existing_key,
            "API key is required; enter a value or press Ctrl-C to cancel."
        );
        Ok(None)
    } else {
        Ok(Some(value.into()))
    }
}

fn config_command(action: &ConfigAction) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut config = Config::load(&cwd, CliOverrides::default())?;
    match action {
        ConfigAction::List => print!("{}", toml::to_string_pretty(&config)?),
        ConfigAction::Get { key } => println!("{}", config_value(&config, key)?),
        ConfigAction::Set { key, value } => {
            config.set(key, value)?;
            let path = config.save_global()?;
            println!("Updated {} in {}", key, path.display());
        }
    }
    Ok(())
}

async fn doctor(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config = Config::load(
        &cwd,
        CliOverrides {
            model: cli.model.as_deref(),
            provider: cli.provider.as_deref(),
        },
    )?;
    println!("✓ configuration: {} / {}", config.provider, config.model);
    match GitRepo::discover(&cwd) {
        Ok(repo) => println!("✓ git repository: {}", repo.root().display()),
        Err(error) => println!("! git repository: {error}"),
    }
    let provider = create_provider(&config)?;
    let models = provider
        .models()
        .await
        .context("Provider connectivity check failed")?;
    println!("✓ provider reachable: {} models returned", models.len());
    println!("✓ credentials: available and never stored in config output");
    Ok(())
}

fn completions(shell: CompletionShell) {
    generate_completions(shell, &mut io::stdout());
}

fn generate_completions(shell: CompletionShell, writer: &mut impl Write) {
    let mut command = Cli::command();
    let name = command.get_name().to_owned();
    match shell {
        CompletionShell::Bash => {
            clap_complete::generate(clap_complete::Shell::Bash, &mut command, name, writer)
        }
        CompletionShell::Elvish => {
            clap_complete::generate(clap_complete::Shell::Elvish, &mut command, name, writer)
        }
        CompletionShell::Fish => {
            clap_complete::generate(clap_complete::Shell::Fish, &mut command, name, writer)
        }
        CompletionShell::PowerShell => {
            clap_complete::generate(clap_complete::Shell::PowerShell, &mut command, name, writer)
        }
        CompletionShell::Zsh => {
            clap_complete::generate(clap_complete::Shell::Zsh, &mut command, name, writer)
        }
    }
}

fn prompt_value(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();
    Ok(if value.is_empty() {
        default.into()
    } else {
        value.into()
    })
}

fn config_value(config: &Config, key: &str) -> Result<String> {
    Ok(match key {
        "provider" => config.provider.clone(),
        "model" => config.model.clone(),
        "base_url" => config.base_url.clone(),
        "language" => config.language.clone(),
        "format" => config.format.clone(),
        "max_input_tokens" => config.max_input_tokens.to_string(),
        "candidates" => config.candidates.to_string(),
        "timeout_seconds" => config.timeout_seconds.to_string(),
        "prompt_file" => config.prompt_file.clone().unwrap_or_default(),
        _ => anyhow::bail!("Unknown configuration key: {key}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_config_values_are_readable_and_unknown_keys_fail() {
        let config = Config {
            prompt_file: Some("prompt.txt".into()),
            ..Config::default()
        };
        for key in [
            "provider",
            "model",
            "base_url",
            "language",
            "format",
            "max_input_tokens",
            "candidates",
            "timeout_seconds",
            "prompt_file",
        ] {
            assert!(!config_value(&config, key).expect("known key").is_empty());
        }
        assert!(config_value(&config, "api_key").is_err());
    }

    #[test]
    fn every_completion_shell_generates_output() {
        for shell in [
            CompletionShell::Bash,
            CompletionShell::Elvish,
            CompletionShell::Fish,
            CompletionShell::PowerShell,
            CompletionShell::Zsh,
        ] {
            let mut output = Vec::new();
            generate_completions(shell, &mut output);
            assert!(!output.is_empty());
        }
    }

    #[test]
    fn ollama_provider_can_be_constructed_without_credentials() {
        let config = Config {
            provider: "ollama".into(),
            model: "qwen3".into(),
            base_url: "http://localhost:11434".into(),
            ..Config::default()
        };
        assert_eq!(
            create_provider(&config).expect("ollama provider").model(),
            "qwen3"
        );
    }

    #[test]
    fn api_key_input_replaces_keeps_or_rejects_as_expected() {
        assert_eq!(
            resolve_api_key_input("  new-secret  ", false)
                .expect("new credential")
                .as_deref(),
            Some("new-secret")
        );
        assert_eq!(
            resolve_api_key_input("", true).expect("keep credential"),
            None
        );
        assert!(resolve_api_key_input("  ", false).is_err());
    }

    #[test]
    fn provider_switches_use_provider_defaults() {
        assert_eq!(
            default_base_url("openai-compatible").expect("OpenAI-compatible URL"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            default_base_url("ollama").expect("Ollama URL"),
            "http://localhost:11434"
        );
        assert!(default_base_url("unsupported").is_err());
    }
}
