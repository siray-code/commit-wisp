//! Sensitive-data preflight for content about to leave the machine.

use std::sync::OnceLock;

use regex::Regex;

/// A redacted finding. The matched secret is deliberately never retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensitiveFinding {
    pub file: String,
    pub line: usize,
    pub rule: &'static str,
}

/// Scans added diff lines and reports only location plus rule name.
#[must_use]
pub fn scan_sensitive(diff: &str) -> Vec<SensitiveFinding> {
    let rules = rules();
    let mut file = "unknown".to_owned();
    let mut findings = Vec::new();
    for (index, line) in diff.lines().enumerate() {
        if let Some(path) = line
            .strip_prefix("diff --git a/")
            .and_then(|rest| rest.split(" b/").nth(1))
        {
            path.clone_into(&mut file);
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            path.clone_into(&mut file);
            continue;
        }
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        for (name, regex) in rules {
            if regex.is_match(line) {
                findings.push(SensitiveFinding {
                    file: file.clone(),
                    line: index + 1,
                    rule: name,
                });
            }
        }
    }
    findings
}

fn rules() -> &'static [(&'static str, Regex)] {
    static RULES: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            (
                "aws-access-key",
                Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid regex"),
            ),
            (
                "private-key",
                Regex::new(r"BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY").expect("valid regex"),
            ),
            (
                "generic-api-key",
                Regex::new(r#"(?i)(?:api[_-]?key|token|secret)\s*[:=]\s*['"]?[A-Za-z0-9_-]{20,}"#)
                    .expect("valid regex"),
            ),
            (
                "github-token",
                Regex::new(r"gh[pousr]_[A-Za-z0-9]{30,}").expect("valid regex"),
            ),
        ]
    })
}
