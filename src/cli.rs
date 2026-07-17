//! Command-line surface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "commit-wisp",
    version,
    about = "Generate and review commit messages from staged Git changes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Override the configured provider for this run.
    #[arg(long, global = true)]
    pub provider: Option<String>,

    /// Override the configured model for this run.
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// Add an instruction to the configured prompt.
    #[arg(long)]
    pub prompt: Option<String>,

    /// Print candidates without creating a commit.
    #[arg(long)]
    pub dry_run: bool,

    /// Permit sending a diff after sensitive content was detected.
    #[arg(long)]
    pub allow_sensitive: bool,

    /// Pass --no-verify to git commit after review.
    #[arg(long)]
    pub no_verify: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Interactively configure a provider, model, and credential.
    Setup(SetupArgs),
    /// Inspect or update global non-secret configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Inspect and manage the commit-message prompt template.
    Prompt {
        #[command(subcommand)]
        action: PromptAction,
    },
    /// Check Git, configuration, credentials, and provider connectivity.
    Doctor,
    /// Generate shell completion scripts.
    Completions { shell: CompletionShell },
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub credential_store: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    List,
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
pub enum PromptAction {
    /// Print the active prompt template and its source.
    Show,
    /// Create and activate a customizable prompt template.
    Init {
        path: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Edit the active prompt template with the configured or system editor.
    Edit,
    /// Restore the built-in prompt without deleting custom files.
    Reset,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}
