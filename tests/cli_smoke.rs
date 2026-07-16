use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_describes_reviewable_staged_commit_workflow() {
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("staged"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn config_list_never_prints_api_key_environment_value() {
    Command::cargo_bin("commit-wisp")
        .expect("binary")
        .args(["config", "list"])
        .env("COMMIT_WISP_API_KEY", "super-secret-value")
        .assert()
        .success()
        .stdout(predicate::str::contains("provider"))
        .stdout(predicate::str::contains("super-secret-value").not());
}
