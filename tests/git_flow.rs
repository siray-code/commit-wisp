use std::{fs, process::Command};

use commit_wisp::git::GitRepo;

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs");
    assert!(status.success());
}

#[test]
fn reads_only_staged_changes_and_commits_reviewed_message() {
    let temp = tempfile::tempdir().expect("temp repository");
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Test User"]);
    git(temp.path(), &["config", "user.email", "test@example.com"]);
    fs::write(temp.path().join("staged.txt"), "staged\n").expect("write staged");
    fs::write(temp.path().join("unstaged.txt"), "first\n").expect("write unstaged");
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "chore: initial"]);

    fs::write(temp.path().join("staged.txt"), "staged change\n").expect("update staged");
    git(temp.path(), &["add", "staged.txt"]);
    fs::write(temp.path().join("unstaged.txt"), "unstaged change\n").expect("update unstaged");

    let repo = GitRepo::discover(temp.path()).expect("discover repository");
    let staged = repo.staged_diff().expect("staged diff");
    assert!(staged.contains("staged change"));
    assert!(!staged.contains("unstaged change"));
    assert!(repo.diff_stats().expect("stats").contains("staged.txt"));
    assert!(repo
        .recent_commits(1)
        .expect("history")
        .contains("chore: initial"));
    assert_eq!(
        repo.root()
            .canonicalize()
            .expect("canonical repository root"),
        temp.path().canonicalize().expect("canonical temp path")
    );

    repo.commit("feat: reviewed message", false)
        .expect("commit succeeds");
    let subject = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(temp.path())
        .output()
        .expect("read log");
    assert_eq!(
        String::from_utf8_lossy(&subject.stdout).trim(),
        "feat: reviewed message"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("unstaged.txt")).unwrap(),
        "unstaged change\n"
    );
}

#[test]
fn rejects_repository_without_staged_changes() {
    let temp = tempfile::tempdir().expect("temp repository");
    git(temp.path(), &["init", "-q"]);
    let repo = GitRepo::discover(temp.path()).expect("discover repository");
    let error = repo.staged_diff().expect_err("empty index should fail");
    assert!(error.to_string().contains("No staged changes"));
}

#[test]
fn tolerates_non_utf8_bytes_in_staged_text_diff() {
    let temp = tempfile::tempdir().expect("temp repository");
    git(temp.path(), &["init", "-q"]);

    fs::write(temp.path().join("generated.txt"), b"valid\n").expect("write initial file");
    git(temp.path(), &["add", "generated.txt"]);
    git(
        temp.path(),
        &[
            "-c",
            "user.name=Test User",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "chore: initial",
        ],
    );

    fs::write(temp.path().join("generated.txt"), b"invalid: \x80\n")
        .expect("write non-UTF-8 content");
    git(temp.path(), &["add", "generated.txt"]);

    let repo = GitRepo::discover(temp.path()).expect("discover repository");
    let staged = repo
        .staged_diff()
        .expect("non-UTF-8 staged diff should be decoded lossily");

    assert!(staged.contains("invalid: \u{fffd}"));
}

#[test]
fn represents_staged_protobuf_binary_without_emitting_its_contents() {
    let temp = tempfile::tempdir().expect("temp repository");
    git(temp.path(), &["init", "-q"]);

    let protobuf = temp.path().join("fixture.pb");
    fs::write(&protobuf, [0x0a, 0x03, b'f', b'o', b'o', 0x00, 0xff])
        .expect("write protobuf fixture");
    git(temp.path(), &["add", "fixture.pb"]);

    let repo = GitRepo::discover(temp.path()).expect("discover repository");
    let staged = repo.staged_diff().expect("binary staged diff");

    assert!(staged.contains("Binary files"));
    assert!(staged.contains("fixture.pb"));
    assert!(!staged.as_bytes().contains(&0xff));
}
