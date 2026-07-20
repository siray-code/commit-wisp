//! Safe orchestration around the user's native Git installation.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
pub struct GitRepo {
    root: PathBuf,
}

impl GitRepo {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .context("Git is not installed or could not be executed")?;
        anyhow::ensure!(output.status.success(), "Not inside a Git repository");
        let root =
            String::from_utf8(output.stdout).context("Git returned a non-UTF-8 repository path")?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn staged_diff(&self) -> Result<String> {
        let output = self.git_output(&[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
        ])?;
        anyhow::ensure!(
            !output.trim().is_empty(),
            "No staged changes. Run git add first."
        );
        Ok(output)
    }

    pub fn diff_stats(&self) -> Result<String> {
        self.git_output(&["diff", "--cached", "--stat", "--no-ext-diff"])
    }

    pub fn recent_commits(&self, count: usize) -> Result<String> {
        self.git_output(&["log", &format!("-{count}"), "--pretty=%s"])
            .or_else(|_| Ok(String::new()))
    }

    pub fn commit(&self, message: &str, no_verify: bool) -> Result<()> {
        anyhow::ensure!(!message.trim().is_empty(), "Commit message cannot be empty");
        let mut file = NamedTempFile::new_in(&self.root)
            .context("Could not create temporary commit message")?;
        file.write_all(message.as_bytes())
            .context("Could not write temporary commit message")?;
        file.flush()
            .context("Could not flush temporary commit message")?;

        let mut command = Command::new("git");
        command.arg("commit").arg("-F").arg(file.path());
        if no_verify {
            command.arg("--no-verify");
        }
        let status = command
            .current_dir(&self.root)
            .status()
            .context("Could not execute git commit")?;
        anyhow::ensure!(
            status.success(),
            "git commit failed; the staged changes were left intact"
        );
        Ok(())
    }

    fn git_output(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .context("Could not execute git")?;
        anyhow::ensure!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
