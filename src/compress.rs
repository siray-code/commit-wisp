//! Deterministic, local-only staged diff compression.

/// Controls how much diff content can be sent to a provider.
#[derive(Debug, Clone)]
pub struct CompressionOptions {
    /// Approximate maximum input tokens allocated to the diff.
    pub max_tokens: usize,
    /// File suffixes whose content is summarized instead of transmitted.
    pub summarized_suffixes: Vec<String>,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            max_tokens: 12_000,
            summarized_suffixes: vec![
                "Cargo.lock".into(),
                "package-lock.json".into(),
                "pnpm-lock.yaml".into(),
                "yarn.lock".into(),
                ".min.js".into(),
                ".min.css".into(),
            ],
        }
    }
}

/// The compressed payload and transparent accounting shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionReport {
    pub content: String,
    pub original_tokens: usize,
    pub estimated_tokens: usize,
    pub omitted_lines: usize,
}

/// Estimates tokens conservatively for provider-independent budgeting.
#[must_use]
pub const fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Compresses each file independently so one large file cannot hide all others.
#[must_use]
pub fn compress_diff(diff: &str, options: &CompressionOptions) -> CompressionReport {
    let sections = split_sections(diff);
    let char_budget = options.max_tokens.saturating_mul(4);
    let per_file = if sections.is_empty() {
        char_budget
    } else {
        char_budget / sections.len()
    };
    let mut content = String::new();
    let mut omitted_lines = 0;

    for section in sections {
        let path = section_path(section).unwrap_or("unknown");
        let summarize = options
            .summarized_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix));
        let (part, omitted) = if summarize {
            summarize_section(section, path)
        } else {
            truncate_section(section, per_file)
        };
        omitted_lines += omitted;
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&part);
    }

    if content.len() > char_budget {
        content.truncate(floor_char_boundary(&content, char_budget));
    }
    CompressionReport {
        original_tokens: estimate_tokens(diff),
        estimated_tokens: estimate_tokens(&content),
        content,
        omitted_lines,
    }
}

fn split_sections(diff: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = diff.match_indices("diff --git ").map(|(i, _)| i).collect();
    if starts.is_empty() {
        return if diff.is_empty() { vec![] } else { vec![diff] };
    }
    starts.push(diff.len());
    starts
        .windows(2)
        .map(|window| &diff[window[0]..window[1]])
        .collect()
}

fn section_path(section: &str) -> Option<&str> {
    section
        .lines()
        .next()?
        .split_whitespace()
        .nth(3)
        .map(|value| value.trim_start_matches("b/"))
}

fn summarize_section(section: &str, path: &str) -> (String, usize) {
    let lines = section.lines().count();
    (
        format!("diff --git a/{path} b/{path}\n# [content omitted: summarized generated/lock file, {lines} diff lines]\n"),
        lines.saturating_sub(1),
    )
}

fn truncate_section(section: &str, budget: usize) -> (String, usize) {
    if section.len() <= budget {
        return (section.to_owned(), 0);
    }
    let marker = "\n# [content omitted: token budget reached]\n";
    let keep = budget.saturating_sub(marker.len());
    let boundary = floor_char_boundary(section, keep);
    let omitted = section[boundary..].lines().count();
    (format!("{}{}", &section[..boundary], marker), omitted)
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_stays_empty() {
        let report = compress_diff("", &CompressionOptions::default());
        assert!(report.content.is_empty());
        assert_eq!(report.estimated_tokens, 0);
    }

    #[test]
    fn keeps_small_diff_unchanged() {
        let diff = "diff --git a/a b/a\n+ok\n";
        let report = compress_diff(diff, &CompressionOptions::default());
        assert_eq!(report.content, diff);
        assert_eq!(report.omitted_lines, 0);
    }
}
