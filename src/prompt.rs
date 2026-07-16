//! Prompt construction with explicit, inspectable context.

use anyhow::Result;

pub struct PromptContext<'a> {
    pub diff: &'a str,
    pub stats: &'a str,
    pub recent_commits: &'a str,
    pub language: &'a str,
    pub format: &'a str,
    pub extra_instruction: Option<&'a str>,
    pub custom_template: Option<&'a str>,
}

const DEFAULT_TEMPLATE: &str = r#"You write precise Git commit messages.
Return JSON only: {"candidates":[{"subject":"type(scope): summary","body":"optional explanation"}]}.
Generate concise {{format}} messages in {{language}}. Subjects must be imperative and at most 72 characters.

Repository statistics:
{{stats}}

Recent commit style:
{{recent_commits}}

Staged diff:
{{diff}}

Additional instruction:
{{extra_instruction}}
"#;

pub fn render_prompt(context: &PromptContext<'_>) -> Result<String> {
    let template = context.custom_template.unwrap_or(DEFAULT_TEMPLATE);
    anyhow::ensure!(
        template.contains("{{diff}}"),
        "custom prompt template must contain {{diff}}"
    );
    let values = [
        ("{{diff}}", context.diff),
        ("{{stats}}", context.stats),
        ("{{recent_commits}}", context.recent_commits),
        ("{{language}}", context.language),
        ("{{format}}", context.format),
        (
            "{{extra_instruction}}",
            context.extra_instruction.unwrap_or("None"),
        ),
    ];
    Ok(values
        .into_iter()
        .fold(template.to_owned(), |output, (key, value)| {
            output.replace(key, value)
        }))
}
