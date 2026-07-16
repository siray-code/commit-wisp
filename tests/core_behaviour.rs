use std::collections::HashMap;

use commit_wisp::{
    compress::{compress_diff, CompressionOptions},
    config::{Config, ConfigSources},
    prompt::{render_prompt, PromptContext},
    provider::parse_candidates,
    security::scan_sensitive,
};

#[test]
fn compresses_large_diff_and_summarizes_lockfiles() {
    let source_lines = (0..100)
        .map(|i| format!("+let value_{i} = {i};"))
        .collect::<Vec<_>>()
        .join("\n");
    let diff = format!(
        "diff --git a/src/lib.rs b/src/lib.rs\n+++ b/src/lib.rs\n@@ -0,0 +1,100 @@\n{source_lines}\n\
         diff --git a/Cargo.lock b/Cargo.lock\n+++ b/Cargo.lock\n@@ -1 +1 @@\n-old\n+new\n"
    );

    let report = compress_diff(
        &diff,
        &CompressionOptions {
            max_tokens: 120,
            ..CompressionOptions::default()
        },
    );

    assert!(report.estimated_tokens <= 120);
    assert!(report.content.contains("src/lib.rs"));
    assert!(report.content.contains("Cargo.lock"));
    assert!(report.content.contains("content omitted"));
    assert!(report.omitted_lines > 0);
}

#[test]
fn sensitive_scan_reports_location_without_secret_value() {
    let secret = "AKIAIOSFODNN7EXAMPLE";
    let input = format!("diff --git a/.env b/.env\n+AWS_KEY={secret}\n");
    let findings = scan_sensitive(&input);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file, ".env");
    assert_eq!(findings[0].rule, "aws-access-key");
    assert!(!format!("{findings:?}").contains(secret));
}

#[test]
fn configuration_precedence_is_cli_env_project_global_default() {
    let mut env = HashMap::new();
    env.insert("COMMIT_WISP_MODEL".into(), "env-model".into());
    env.insert("COMMIT_WISP_PROVIDER".into(), "ollama".into());
    env.insert(
        "COMMIT_WISP_BASE_URL".into(),
        "http://localhost:11434".into(),
    );
    env.insert("COMMIT_WISP_LANGUAGE".into(), "fr".into());
    env.insert("COMMIT_WISP_MAX_INPUT_TOKENS".into(), "4096".into());
    let config = Config::resolve(ConfigSources {
        global_toml: Some("model = 'global-model'\nmax_input_tokens = 9000"),
        project_toml: Some("model = 'project-model'\nlanguage = 'zh'"),
        env: &env,
        cli_model: Some("cli-model"),
        cli_provider: None,
    })
    .expect("valid configuration");

    assert_eq!(config.model, "cli-model");
    assert_eq!(config.language, "fr");
    assert_eq!(config.max_input_tokens, 4096);
    assert_eq!(config.provider, "ollama");
    assert_eq!(config.base_url, "http://localhost:11434");
}

#[test]
fn prompt_contains_context_and_additional_instruction() {
    let rendered = render_prompt(&PromptContext {
        diff: "diff --git a/a.rs b/a.rs\n+hello",
        stats: "a.rs | 1 +",
        recent_commits: "feat: previous",
        language: "en",
        format: "conventional",
        extra_instruction: Some("Focus on the public API"),
        custom_template: None,
    })
    .expect("prompt renders");

    assert!(rendered.contains("diff --git"));
    assert!(rendered.contains("feat: previous"));
    assert!(rendered.contains("Focus on the public API"));
    assert!(rendered.contains("JSON"));
}

#[test]
fn parses_structured_and_fenced_candidate_responses() {
    let response = r#"```json
        {"candidates":[{"subject":"feat(cli): add review flow","body":"Adds an interactive review step."}]}
    ```"#;

    let candidates = parse_candidates(response).expect("candidate response");
    assert_eq!(candidates[0].subject, "feat(cli): add review flow");
    assert_eq!(
        candidates[0].body.as_deref(),
        Some("Adds an interactive review step.")
    );
}

#[test]
fn validates_configuration_and_custom_prompt_contracts() {
    let env = HashMap::new();
    let invalid = Config::resolve(ConfigSources {
        global_toml: Some("provider = 'unknown'"),
        project_toml: None,
        env: &env,
        cli_model: None,
        cli_provider: None,
    });
    assert!(invalid.is_err());

    let missing_diff = render_prompt(&PromptContext {
        diff: "change",
        stats: "",
        recent_commits: "",
        language: "en",
        format: "conventional",
        extra_instruction: None,
        custom_template: Some("No staged placeholder"),
    });
    assert!(missing_diff.is_err());
}

#[test]
fn detects_multiple_secret_families_without_retaining_values() {
    let diff = "diff --git a/secrets.txt b/secrets.txt\n+++ b/secrets.txt\n+-----BEGIN PRIVATE KEY-----\n+token=abcdefghijklmnopqrstuvwxyz123456\n+ghp_abcdefghijklmnopqrstuvwxyz1234567890\n";
    let findings = scan_sensitive(diff);
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().all(|finding| finding.file == "secrets.txt"));
    assert!(!format!("{findings:?}").contains("abcdefghijklmnopqrstuvwxyz"));
}
