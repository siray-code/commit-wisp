use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command as ProcessCommand,
    thread,
};

use assert_cmd::Command;
use predicates::prelude::*;

fn git(dir: &Path, args: &[&str]) {
    assert!(ProcessCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git")
        .success());
}

fn staged_repo(content: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp repository");
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("change.rs"), content).expect("write staged file");
    git(temp.path(), &["add", "change.rs"]);
    temp
}

fn provider_server(requests: usize) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider");
    let address = listener.local_addr().expect("provider address");
    let handle = thread::spawn(move || {
        (0..requests)
            .map(|_| {
                let (mut stream, _) = listener.accept().expect("accept provider request");
                let mut input = [0_u8; 32_768];
                let count = stream.read(&mut input).expect("read request");
                let request = String::from_utf8_lossy(&input[..count]).into_owned();
                let response = if request.starts_with("GET /models") {
                    r#"{"data":[{"id":"test-model"},{"id":"other-model"}]}"#
                } else {
                    r#"{"choices":[{"message":{"content":"{\"candidates\":[{\"subject\":\"feat(cli): generated safely\",\"body\":\"Describes staged changes.\"},{\"subject\":\"feat: expose generated feature\",\"body\":null},{\"subject\":\"feat(api): add public feature\",\"body\":null}]}"}}]}"#
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).expect("write response");
                request
            })
            .collect()
    });
    (format!("http://{address}"), handle)
}

#[test]
fn dry_run_covers_staged_git_prompt_provider_and_output_without_commit() {
    let repo = staged_repo("pub fn feature() {}\n");
    let config_home = tempfile::tempdir().expect("config home");
    let config_dir = config_home.path().join("commit-wisp");
    fs::create_dir(&config_dir).expect("create config directory");
    fs::write(config_dir.join("config.toml"), "candidates = 3\n").expect("write config");
    let (base_url, handle) = provider_server(2);
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .current_dir(repo.path())
        .args(["--dry-run", "--model", "test-model"])
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("COMMIT_WISP_BASE_URL", base_url)
        .env("COMMIT_WISP_API_KEY", "test-only-key")
        .assert()
        .success()
        .stdout(predicate::str::contains("feat(cli): generated safely"))
        .stdout(predicate::str::contains("Staged diff tokens"));
    let requests = handle.join().expect("provider thread");
    assert!(requests
        .iter()
        .any(|request| request.starts_with("GET /models")));
    assert!(requests
        .iter()
        .any(|request| request.starts_with("POST /chat/completions")));

    let status = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo.path())
        .output()
        .expect("git status");
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        "A  change.rs"
    );
}

#[test]
fn sensitive_staged_content_blocks_before_credentials_or_network() {
    let repo = staged_repo("const KEY: &str = \"AKIAIOSFODNN7EXAMPLE\";\n");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .current_dir(repo.path())
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Sensitive content detected"))
        .stderr(predicate::str::contains("AKIAIOSFODNN7EXAMPLE").not());
}

#[test]
fn doctor_checks_repository_credentials_and_provider_models() {
    let repo = staged_repo("fn main() {}\n");
    let (base_url, handle) = provider_server(1);
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .current_dir(repo.path())
        .arg("doctor")
        .env("COMMIT_WISP_BASE_URL", base_url)
        .env("COMMIT_WISP_API_KEY", "test-only-key")
        .assert()
        .success()
        .stdout(predicate::str::contains("provider reachable"))
        .stdout(predicate::str::contains("credentials"));
    assert_eq!(handle.join().expect("provider thread").len(), 1);
}

#[test]
fn global_config_set_get_and_completion_generation_work() {
    let home = tempfile::tempdir().expect("config home");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "set", "language", "zh"])
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success();
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "get", "language"])
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout("zh\n");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_commit-wisp"));
}

#[test]
fn setup_keeps_historical_defaults_and_saves_model_when_discovery_fails() {
    let home = tempfile::tempdir().expect("config home");
    for (key, value) in [
        ("provider", "ollama"),
        ("base_url", "http://127.0.0.1:1"),
        ("model", "historical-model"),
    ] {
        Command::cargo_bin("commit-wisp")
            .expect("binary")
            .args(["config", "set", key, value])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path())
            .assert()
            .success();
    }

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .arg("setup")
        .write_stdin("\n\nmanual-model\n")
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Provider (openai-compatible/ollama) [ollama]",
        ))
        .stdout(predicate::str::contains("Base URL [http://127.0.0.1:1]"))
        .stdout(predicate::str::contains("Model [historical-model]"))
        .stderr(predicate::str::contains(
            "warning: Could not list provider models",
        ));

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "get", "model"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout("manual-model\n");
}

#[test]
fn setup_reuses_environment_key_without_exposing_or_persisting_it() {
    let home = tempfile::tempdir().expect("config home");
    let (base_url, handle) = provider_server(1);
    let secret = "test-only-environment-secret";

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args([
            "setup",
            "--provider",
            "openai-compatible",
            "--base-url",
            &base_url,
            "--model",
            "test-model",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("COMMIT_WISP_API_KEY", secret)
        .assert()
        .success()
        .stdout(predicate::str::contains(secret).not())
        .stderr(predicate::str::contains(secret).not());
    assert_eq!(handle.join().expect("provider thread").len(), 1);

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "list"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env_remove("COMMIT_WISP_API_KEY")
        .assert()
        .success()
        .stdout(predicate::str::contains(secret).not());
}

#[test]
fn credential_store_configuration_is_visible_without_exposing_credentials() {
    let home = tempfile::tempdir().expect("config home");
    let secret = "test-only-file-credential";

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "set", "credential_store", "file"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success();
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "list"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("COMMIT_WISP_API_KEY", secret)
        .assert()
        .success()
        .stdout(predicate::str::contains("credential_store = \"file\""))
        .stdout(predicate::str::contains(secret).not());
}

#[test]
fn prompt_commands_initialize_show_and_reset_a_global_template() {
    let home = tempfile::tempdir().expect("config home");

    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "show"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Prompt source: built-in"))
        .stdout(predicate::str::contains("{{diff}}"));
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "init"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("prompt.txt"));
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "init"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "show"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Prompt source:").and(predicate::str::contains("prompt.txt")),
        );
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "reset"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success();
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "show"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Prompt source: built-in"));
}

#[cfg(unix)]
#[test]
fn prompt_edit_validates_and_restores_invalid_edits() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("config home");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "init"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success();

    let editor = home.path().join("editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'Custom {{diff}} template\\n' > \"$1\"\n",
    )
    .expect("write editor");
    fs::set_permissions(&editor, fs::Permissions::from_mode(0o700)).expect("editor permissions");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "edit"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("EDITOR", &editor)
        .assert()
        .success();

    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'invalid template\\n' > \"$1\"\n",
    )
    .expect("rewrite editor");
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "edit"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("EDITOR", &editor)
        .assert()
        .failure()
        .stderr(predicate::str::contains("previous template was restored"));
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["prompt", "show"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Custom {{diff}} template"));
}
