//! Provider-independent commit candidate types and response parsing.

use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub subject: String,
    #[serde(default)]
    pub body: Option<String>,
}

impl Candidate {
    #[must_use]
    pub fn message(&self) -> String {
        self.body
            .as_ref()
            .filter(|body| !body.trim().is_empty())
            .map_or_else(
                || self.subject.clone(),
                |body| format!("{}\n\n{}", self.subject, body.trim()),
            )
    }
}

#[derive(Deserialize)]
struct CandidateEnvelope {
    candidates: Vec<Candidate>,
}

pub fn parse_candidates(raw: &str) -> Result<Vec<Candidate>> {
    let trimmed = raw.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    let mut envelope: CandidateEnvelope =
        serde_json::from_str(json).context("Provider did not return valid candidate JSON")?;
    envelope
        .candidates
        .retain(|candidate| !candidate.subject.trim().is_empty());
    anyhow::ensure!(
        !envelope.candidates.is_empty(),
        "Provider returned no commit candidates"
    );
    for candidate in &envelope.candidates {
        anyhow::ensure!(
            candidate.subject.chars().count() <= 120,
            "Provider returned an excessively long subject"
        );
        anyhow::ensure!(
            !candidate.subject.contains('\n'),
            "Commit subject must be one line"
        );
    }
    Ok(envelope.candidates)
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, count: usize) -> Result<Vec<Candidate>>;
    async fn models(&self) -> Result<Vec<String>>;
    fn model(&self) -> &str;
}

pub struct OpenAiProvider {
    client: Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(
        base_url: String,
        model: String,
        api_key: Option<String>,
        timeout_seconds: u64,
    ) -> Result<Self> {
        validate_endpoint(&base_url)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').into(),
            model,
            api_key,
        })
    }

    fn authenticated(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    n: usize,
    temperature: f32,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiModels {
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(&self, prompt: &str, count: usize) -> Result<Vec<Candidate>> {
        let body = OpenAiRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: prompt,
            }],
            n: count,
            temperature: 0.2,
        };
        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);
        let response: OpenAiResponse = self
            .authenticated(request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Invalid OpenAI-compatible response")?;
        let mut candidates = Vec::new();
        for choice in response.choices {
            candidates.extend(parse_candidates(&choice.message.content)?);
        }
        anyhow::ensure!(!candidates.is_empty(), "Provider returned no choices");
        Ok(candidates)
    }

    async fn models(&self) -> Result<Vec<String>> {
        let request = self.client.get(format!("{}/models", self.base_url));
        let response: OpenAiModels = self
            .authenticated(request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut models: Vec<_> = response.data.into_iter().map(|model| model.id).collect();
        models.sort();
        Ok(models)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String, timeout_seconds: u64) -> Result<Self> {
        validate_endpoint(&base_url)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').into(),
            model,
        })
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
    format: &'a str,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OllamaModels {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn generate(&self, prompt: &str, _count: usize) -> Result<Vec<Candidate>> {
        let body = OllamaRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: prompt,
            }],
            stream: false,
            format: "json",
        };
        let response: OllamaResponse = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("Invalid Ollama response")?;
        parse_candidates(&response.message.content)
    }

    async fn models(&self) -> Result<Vec<String>> {
        let response: OllamaModels = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut models: Vec<_> = response
            .models
            .into_iter()
            .map(|model| model.name)
            .collect();
        models.sort();
        Ok(models)
    }

    fn model(&self) -> &str {
        &self.model
    }
}

fn validate_endpoint(base_url: &str) -> Result<()> {
    anyhow::ensure!(
        base_url.starts_with("https://")
            || base_url.starts_with("http://localhost")
            || base_url.starts_with("http://127.0.0.1"),
        "Provider endpoint must use HTTPS; plain HTTP is allowed only for localhost"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_message_omits_empty_body() {
        let candidate = Candidate {
            subject: "fix: bug".into(),
            body: Some("  ".into()),
        };
        assert_eq!(candidate.message(), "fix: bug");
    }

    #[test]
    fn rejects_empty_candidates() {
        assert!(parse_candidates(r#"{"candidates":[]}"#).is_err());
    }
}
