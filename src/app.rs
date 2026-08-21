//! Application orchestration.

use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

use crate::{
    cli::{Cli, Commands, CompletionShell, ConfigAction, PromptAction, SetupArgs},
    compress::{compress_diff, CompressionOptions},
    config::{global_config_path, CliOverrides, Config},
    git::GitRepo,
    prompt::{default_template, render_prompt, validate_template, PromptContext},
    provider::{
        generate_candidates_parallel_traced, validate_endpoint, GenerationTiming, LlmProvider,
        OllamaProvider, OpenAiProvider,
    },
    secret::SecretStore,
    security::scan_sensitive,
    tui::{self, ReviewAction, ReviewInput},
};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Commands::Setup(args)) => setup(args).await,
        Some(Commands::Config { action }) => config_command(action),
        Some(Commands::Prompt { action }) => prompt_command(action),
        Some(Commands::Doctor) => doctor(&cli).await,
        Some(Commands::Update) => crate::update::update().await,
        Some(Commands::Completions { shell }) => {
            completions(*shell);
            Ok(())
        }
        None => generate_and_review(&cli).await,
    }
}

/// How long the model-list request may outlive generation before we fall back
/// to the configured model. Keeps a hung `/models` endpoint off the critical
/// path while giving a healthy one (TCP + TLS handshake included) time to win.
const MODELS_GRACE: Duration = Duration::from_millis(500);

fn report_phase(cli: &Cli, name: &str, elapsed: Duration, detail: Option<String>) {
    if !cli.verbose {
        return;
    }
    match detail {
        Some(detail) => eprintln!(
            "commit-wisp: {name}: {detail} ({:.2}s)",
            elapsed.as_secs_f64()
        ),
        None => eprintln!("commit-wisp: {name}: {:.2}s", elapsed.as_secs_f64()),
    }
}

fn report_generation_timings(cli: &Cli, timings: &[GenerationTiming]) {
    if !cli.verbose {
        return;
    }
    for timing in timings {
        let seconds = |duration: Option<Duration>| {
            duration.map_or_else(
                || "n/a".into(),
                |value| format!("{:.2}s", value.as_secs_f64()),
            )
        };
        eprintln!(
            "commit-wisp: candidate #{}: headers={} first-content={} complete={}",
            timing.request_index,
            seconds(timing.response_headers),
            seconds(timing.first_content),
            seconds(timing.completed),
        );
    }
}

async fn generate_and_review(cli: &Cli) -> Result<()> {
    let total_start = Instant::now();
    let cwd = std::env::current_dir()?;
    let mut config = Config::load(
        &cwd,
        CliOverrides {
            model: cli.model.as_deref(),
            provider: cli.provider.as_deref(),
        },
    )?;
    let repo = GitRepo::discover(&cwd)?;
    let git_start = Instant::now();
    let diff = repo.staged_diff()?;
    let stats = repo.diff_stats()?;
    let recent = repo.recent_commits(8)?;
    report_phase(cli, "git", git_start.elapsed(), None);
    let findings = scan_sensitive(&diff);
    if !findings.is_empty() && !cli.allow_sensitive {
        let locations = findings
            .iter()
            .map(|finding| format!("{}:{} ({})", finding.file, finding.line, finding.rule))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("Sensitive content detected; nothing was sent: {locations}. Inspect the staged diff or explicitly use --allow-sensitive.");
    }
    let compress_start = Instant::now();
    let compression = compress_diff(
        &diff,
        &CompressionOptions {
            max_tokens: config.max_input_tokens.saturating_sub(1_200),
            ..CompressionOptions::default()
        },
    );
    report_phase(cli, "compress", compress_start.elapsed(), None);
    let prompt_start = Instant::now();
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
        candidate_count: 1,
        extra_instruction: cli.prompt.as_deref(),
        custom_template: custom_template.as_deref(),
    })?;
    report_phase(cli, "prompt", prompt_start.elapsed(), None);

    if cli.verbose {
        eprintln!(
            "commit-wisp: provider={} base_url={} model={}",
            config.provider, config.base_url, config.model
        );
    }
    let interactive = !cli.dry_run && io::stdin().is_terminal() && io::stdout().is_terminal();
    let mut provider = create_provider(&config)?;
    let mut models_cache: Option<Vec<String>> = None;
    loop {
        if provider.model() != config.model {
            provider = create_provider(&config)?;
        }
        eprintln!(
            "commit-wisp: generating candidates with {} via {}…",
            config.model, config.base_url
        );
        let generate_start = Instant::now();
        let progress = if interactive {
            Some(tokio::spawn(async {
                let start = Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    eprint!(
                        "\rcommit-wisp: generating… {:.1}s  ",
                        start.elapsed().as_secs_f64()
                    );
                    let _ = io::stderr().flush();
                }
            }))
        } else {
            None
        };
        let (candidates, models) = if interactive && models_cache.is_none() {
            let models_future = async {
                let started = Instant::now();
                (provider.models().await, started.elapsed())
            };
            tokio::pin!(models_future);
            let (candidates, models_result) = tokio::join!(
                generate_candidates_parallel_traced(provider.as_ref(), &prompt, config.candidates),
                tokio::time::timeout(MODELS_GRACE, &mut models_future),
            );
            report_phase(
                cli,
                "models()",
                match &models_result {
                    Ok((_, elapsed)) => *elapsed,
                    Err(_) => MODELS_GRACE,
                },
                match &models_result {
                    Ok((Ok(models), _)) => Some(format!("{} models", models.len())),
                    Ok((Err(error), _)) => Some(format!("failed: {error}")),
                    Err(_) => Some("timed out; fell back".into()),
                },
            );
            match models_result {
                Ok((Ok(models), _)) => {
                    models_cache = Some(models.clone());
                    (candidates, models)
                }
                Ok((Err(_), _)) | Err(_) => (candidates, vec![config.model.clone()]),
            }
        } else {
            let models = models_cache
                .clone()
                .unwrap_or_else(|| vec![config.model.clone()]);
            if cli.verbose {
                let detail = if models_cache.is_some() {
                    "cached"
                } else {
                    "skipped (non-interactive)"
                };
                eprintln!("commit-wisp: models(): {detail}");
            }
            (
                generate_candidates_parallel_traced(provider.as_ref(), &prompt, config.candidates)
                    .await,
                models,
            )
        };
        if let Some(task) = progress {
            task.abort();
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
        }
        let (mut candidates, timings) = candidates?;
        report_generation_timings(cli, &timings);
        report_phase(
            cli,
            "generate()",
            generate_start.elapsed(),
            Some(format!("{} candidates", candidates.len())),
        );
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
            report_phase(cli, "total", total_start.elapsed(), None);
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
                report_phase(cli, "total", total_start.elapsed(), None);
                return Ok(());
            }
            ReviewAction::Regenerate => {}
            ReviewAction::ChangeModel(model) => config.model = model,
            ReviewAction::Cancel => {
                println!("Cancelled; staged changes were not modified.");
                report_phase(cli, "total", total_start.elapsed(), None);
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
            let key = SecretStore::get(&config.credential_store, &config.provider)?;
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

fn setup_banner() -> &'static str {
    r#"
   ___ ___  _ __ ___  _ __ ___ (_) |_      __      ___ ___ _ __
  / __/ _ \| '_ ` _ \| '_ ` _ \| | __|____\ \ /\ / / / __| '_ \
 | (_| (_) | | | | | | | | | | | | ||_____|\ V  V /| \__ \ |_) |
  \___\___/|_| |_| |_|_| |_| |_|_|_|\__|     \_/\_/ |_|___/ .__/
                                                            |_|
                         commit-wisp
"#
}

async fn setup(args: &SetupArgs) -> Result<()> {
    println!("{}", setup_banner());
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

    if config.provider == "openai-compatible" {
        let credential_store = args.credential_store.clone().unwrap_or(prompt_value(
            "Credential store (system/file)",
            &config.credential_store,
        )?);
        config.set("credential_store", &credential_store)?;
        if config.credential_store == "file" {
            eprintln!(
                "warning: file credentials are plaintext protected by user-only file permissions"
            );
        }
    } else if let Some(credential_store) = &args.credential_store {
        config.set("credential_store", credential_store)?;
    }

    // Keep confirmed non-secret values even if credential setup fails later.
    let path = config.save_global()?;

    if config.provider == "openai-compatible" {
        let has_existing_key =
            SecretStore::get(&config.credential_store, &config.provider)?.is_some();
        if let Some(key) = prompt_api_key(has_existing_key)? {
            SecretStore::set(&config.credential_store, &config.provider, &key)?;
        }
        #[cfg(target_os = "macos")]
        if config.credential_store == "system" {
            println!(
                "macOS Keychain: choose Always Allow for stable installed binaries; local rebuilds may require authorization again."
            );
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
    print_prompt_summary(&config);
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

fn prompt_command(action: &PromptAction) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut config = Config::load(&cwd, CliOverrides::default())?;
    match action {
        PromptAction::Show => {
            let (source, template) = active_prompt_template(&config, &cwd)?;
            println!("Prompt source: {source}\n");
            print!("{template}");
        }
        PromptAction::Init { path, force } => {
            let path = initialize_prompt(&mut config, &cwd, path.as_deref(), *force)?;
            println!(
                "Initialized and activated prompt template at {}",
                path.display()
            );
        }
        PromptAction::Edit => {
            let path = match config.prompt_file.as_deref() {
                Some(path) => resolve_prompt_path(&cwd, Path::new(path)),
                None => {
                    let path = default_prompt_path()?;
                    if path.exists() {
                        config.set("prompt_file", &path.to_string_lossy())?;
                        config.save_global()?;
                        path
                    } else {
                        initialize_prompt(&mut config, &cwd, None, false)?
                    }
                }
            };
            edit_prompt(&path)?;
            println!("Updated prompt template at {}", path.display());
        }
        PromptAction::Reset => {
            config.set("prompt_file", "")?;
            let path = config.save_global()?;
            println!(
                "Restored the built-in prompt; custom files were not deleted ({})",
                path.display()
            );
        }
    }
    Ok(())
}

fn initialize_prompt(
    config: &mut Config,
    cwd: &Path,
    requested_path: Option<&Path>,
    force: bool,
) -> Result<PathBuf> {
    let path = requested_path
        .map(|path| resolve_prompt_path(cwd, path))
        .map_or_else(default_prompt_path, Ok)?;
    if path.exists() && !force {
        anyhow::bail!(
            "Prompt template already exists at {}; use --force to overwrite it",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Could not create prompt directory")?;
    }
    fs::write(&path, default_template()).context("Could not write prompt template")?;
    config.set("prompt_file", &path.to_string_lossy())?;
    config.save_global()?;
    Ok(path)
}

fn default_prompt_path() -> Result<PathBuf> {
    Ok(global_config_path()?.with_file_name("prompt.txt"))
}

fn edit_prompt(path: &Path) -> Result<()> {
    let original = fs::read_to_string(path).context("Could not read prompt template")?;
    let (program, arguments) = prompt_editor();
    let status = ProcessCommand::new(program)
        .args(arguments)
        .arg(path)
        .status()
        .context("Could not launch prompt editor")?;
    anyhow::ensure!(status.success(), "Prompt editor exited unsuccessfully");
    let edited = fs::read_to_string(path).context("Could not read edited prompt template")?;
    if let Err(error) = validate_template(&edited) {
        fs::write(path, original).context("Could not restore the previous prompt template")?;
        return Err(error).context("Invalid prompt edit; previous template was restored");
    }
    Ok(())
}

fn prompt_editor() -> (String, Vec<String>) {
    if let Ok(output) = ProcessCommand::new("git")
        .args(["var", "GIT_EDITOR"])
        .output()
    {
        if output.status.success() {
            let configured = String::from_utf8_lossy(&output.stdout);
            if let Some(editor) = parse_editor_command(configured.trim()) {
                return editor;
            }
        }
    }

    #[cfg(target_os = "macos")]
    return ("open".into(), vec!["-W".into(), "-t".into()]);
    #[cfg(target_os = "windows")]
    return ("notepad.exe".into(), Vec::new());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    ("vi".into(), Vec::new())
}

fn parse_editor_command(value: &str) -> Option<(String, Vec<String>)> {
    let mut parts = value.split_whitespace();
    let program = parts.next()?.to_owned();
    Some((program, parts.map(str::to_owned).collect()))
}

fn active_prompt_template(config: &Config, cwd: &Path) -> Result<(String, String)> {
    match config.prompt_file.as_deref() {
        Some(path) => {
            let path = resolve_prompt_path(cwd, Path::new(path));
            let template = fs::read_to_string(&path).context("Could not read prompt_file")?;
            validate_template(&template)?;
            Ok((path.display().to_string(), template))
        }
        None => Ok(("built-in".into(), default_template().into())),
    }
}

fn resolve_prompt_path(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.into()
    } else {
        cwd.join(path)
    }
}

fn print_prompt_summary(config: &Config) {
    match config.prompt_file.as_deref() {
        Some(path) => println!("Prompt template: {path}"),
        None => println!("Prompt template: built-in"),
    }
    println!("Manage templates with `commit-wisp prompt show|init|edit|reset`; use --prompt for a one-time instruction.");
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
        "credential_store" => config.credential_store.clone(),
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
            "credential_store",
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
    fn setup_banner_identifies_commit_wisp() {
        let banner = setup_banner();

        assert!(banner.contains("commit-wisp"));
        assert!(banner.lines().count() >= 4);
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

    #[test]
    fn parses_editor_commands_with_arguments() {
        assert_eq!(
            parse_editor_command("code --wait"),
            Some(("code".into(), vec!["--wait".into()]))
        );
        assert_eq!(parse_editor_command("  "), None);
    }
}
