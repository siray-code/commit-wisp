//! Prompt construction with explicit, inspectable context.

use anyhow::Result;

pub struct PromptContext<'a> {
    pub diff: &'a str,
    pub stats: &'a str,
    pub recent_commits: &'a str,
    pub language: &'a str,
    pub format: &'a str,
    pub candidate_count: usize,
    pub extra_instruction: Option<&'a str>,
    pub custom_template: Option<&'a str>,
}

const DEFAULT_TEMPLATE: &str = r#"Generate exactly {{candidate_count}} Git commit message candidate based only on the staged changes.
Each subject must use this format: <type>(<scope>): <Chinese summary>.
The type must be one of feat, fix, build, chore, ci, docs, style, refactor, perf, test.
The scope is optional; when present, keep it in English lowercase.
Write the summary in Simplified Chinese using a concise verb-object structure.
Each candidate must include a non-empty Simplified Chinese body with 2 to 4 concise bullet points.
Use the body to explain the concrete changes, motivation, user-visible impact, and verification when the staged changes provide evidence for them.
Do not repeat the subject in the body, invent facts, or mention details that cannot be inferred from the staged changes.
Return JSON only in this shape: {"candidates":[{"subject":"feat(scope): 中文动宾摘要","body":"- 说明核心改动\\n- 说明影响或验证"}]}.

Repository statistics:
{{stats}}

Recent commit style:
{{recent_commits}}

Staged diff:
{{diff}}

Additional instruction:
{{extra_instruction}}
"#;

#[must_use]
pub fn default_template() -> &'static str {
    DEFAULT_TEMPLATE
}

pub fn validate_template(template: &str) -> Result<()> {
    anyhow::ensure!(
        template.contains("{{diff}}"),
        "custom prompt template must contain {{diff}}"
    );
    Ok(())
}

pub fn render_prompt(context: &PromptContext<'_>) -> Result<String> {
    let template = context.custom_template.unwrap_or(DEFAULT_TEMPLATE);
    validate_template(template)?;
    let candidate_count = context.candidate_count.to_string();
    let values = [
        ("{{diff}}", context.diff),
        ("{{stats}}", context.stats),
        ("{{recent_commits}}", context.recent_commits),
        ("{{language}}", context.language),
        ("{{format}}", context.format),
        ("{{candidate_count}}", candidate_count.as_str()),
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
