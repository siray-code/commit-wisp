//! Provider-independent commit candidate types and response parsing.

use std::{collections::HashSet, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{future::try_join_all, StreamExt};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    validate_candidates(&envelope.candidates)?;
    Ok(envelope.candidates)
}

fn validate_candidates(candidates: &[Candidate]) -> Result<()> {
    let mut unique = HashSet::with_capacity(candidates.len());
    for candidate in candidates {
        anyhow::ensure!(
            candidate.subject.chars().count() <= 72,
            "Commit subject must be at most 72 characters"
        );
        anyhow::ensure!(
            !candidate.subject.contains('\n'),
            "Commit subject must be one line"
        );
        if let Some(body) = &candidate.body {
            anyhow::ensure!(
                !body.trim().is_empty(),
                "Commit body must be null or contain non-whitespace text"
            );
        }
        anyhow::ensure!(
            unique.insert((&candidate.subject, candidate.body.as_deref())),
            "Provider returned duplicate commit candidates"
        );
    }
    Ok(())
}

fn validate_candidate_count(candidates: Vec<Candidate>, expected: usize) -> Result<Vec<Candidate>> {
    anyhow::ensure!(
        candidates.len() == expected,
        "Provider must return exactly {expected} candidates, got {}",
        candidates.len()
    );
    validate_candidates(&candidates)?;
    Ok(candidates)
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate(&self, prompt: &str, count: usize) -> Result<Vec<Candidate>>;
    async fn generate_traced(
        &self,
        prompt: &str,
        count: usize,
        timing: &mut GenerationTiming,
    ) -> Result<Vec<Candidate>> {
        let result = self.generate(prompt, count).await;
        timing.complete();
        result
    }
    async fn models(&self) -> Result<Vec<String>>;
    fn model(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct GenerationTiming {
    pub request_index: usize,
    pub response_headers: Option<Duration>,
    pub first_content: Option<Duration>,
    pub completed: Option<Duration>,
    started: std::time::Instant,
}

impl GenerationTiming {
    fn new(request_index: usize) -> Self {
        Self {
            request_index,
            response_headers: None,
            first_content: None,
            completed: None,
            started: std::time::Instant::now(),
        }
    }

    fn mark_headers(&mut self) {
        self.response_headers.get_or_insert(self.started.elapsed());
    }

    fn mark_first_content(&mut self) {
        self.first_content.get_or_insert(self.started.elapsed());
    }

    fn complete(&mut self) {
        self.completed.get_or_insert(self.started.elapsed());
    }
}

pub async fn generate_candidates_parallel(
    provider: &dyn LlmProvider,
    prompt: &str,
    count: usize,
) -> Result<Vec<Candidate>> {
    anyhow::ensure!(count > 0, "Candidate count must be greater than zero");

    let batches = try_join_all((0..count).map(|index| {
        let prompt = format!(
            "{prompt}\n\nThis is candidate variation {} of {count}. Choose a materially distinct valid focus, scope, or level of detail from the other variations.",
            index + 1
        );
        async move {
            provider
                .generate(&prompt, 1)
                .await
                .with_context(|| format!("Candidate request {} failed", index + 1))
        }
    }))
    .await?;

    let candidates: Vec<_> = batches.into_iter().flatten().collect();
    validate_candidates(&candidates)?;
    validate_candidate_count(candidates, count)
}

pub async fn generate_candidates_parallel_traced(
    provider: &dyn LlmProvider,
    prompt: &str,
    count: usize,
) -> Result<(Vec<Candidate>, Vec<GenerationTiming>)> {
    anyhow::ensure!(count > 0, "Candidate count must be greater than zero");

    let generated = try_join_all((0..count).map(|index| {
        let prompt = format!(
            "{prompt}\n\nThis is candidate variation {} of {count}. Choose a materially distinct valid focus, scope, or level of detail from the other variations.",
            index + 1
        );
        async move {
            let mut timing = GenerationTiming::new(index + 1);
            let result = provider
                .generate_traced(&prompt, 1, &mut timing)
                .await
                .with_context(|| format!("Candidate request {} failed", index + 1))?;
            Ok::<_, anyhow::Error>((result, timing))
        }
    }))
    .await?;

    let (batches, timings): (Vec<_>, Vec<_>) = generated.into_iter().unzip();
    let candidates: Vec<_> = batches.into_iter().flatten().collect();
    validate_candidates(&candidates)?;
    Ok((validate_candidate_count(candidates, count)?, timings))
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
    temperature: f32,
    stream: bool,
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

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    #[serde(default)]
    delta: OpenAiStreamDelta,
}

#[derive(Deserialize, Default)]
struct OpenAiStreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate(&self, prompt: &str, count: usize) -> Result<Vec<Candidate>> {
        let prompt = candidate_prompt(prompt, count);
        let body = OpenAiRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: &prompt,
            }],
            temperature: 0.2,
            stream: true,
        };
        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);
        let response = self
            .authenticated(request)
            .send()
            .await
            .context("Could not reach the OpenAI-compatible provider")?;
        let response = ensure_success(response).await?;
        let content = if is_streaming_content_type(&response) {
            consume_sse(response)
                .await
                .context("Failed to read streaming response; provider may not support SSE")?
        } else {
            // Gateways that ignore `stream: true` return a plain JSON body;
            // parse it exactly like the non-streaming path of old.
            let response: OpenAiResponse = response
                .json()
                .await
                .context("Invalid OpenAI-compatible response")?;
            let mut content = String::new();
            for choice in response.choices {
                content.push_str(&choice.message.content);
            }
            content
        };
        let candidates = parse_candidates(&content)?;
        validate_candidate_count(candidates, count)
    }

    async fn generate_traced(
        &self,
        prompt: &str,
        count: usize,
        timing: &mut GenerationTiming,
    ) -> Result<Vec<Candidate>> {
        let prompt = candidate_prompt(prompt, count);
        let body = OpenAiRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: &prompt,
            }],
            temperature: 0.2,
            stream: true,
        };
        let request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);
        let response = self
            .authenticated(request)
            .send()
            .await
            .context("Could not reach OpenAI-compatible provider")?;
        timing.mark_headers();
        let response = ensure_success(response).await?;
        let content = if is_streaming_content_type(&response) {
            consume_sse_traced(response, timing)
                .await
                .context("Failed to read streaming response; provider may not support SSE")?
        } else {
            let response: OpenAiResponse = response
                .json()
                .await
                .context("Invalid OpenAI-compatible response")?;
            timing.mark_first_content();
            let mut content = String::new();
            for choice in response.choices {
                content.push_str(&choice.message.content);
            }
            content
        };
        let candidates = parse_candidates(&content)?;
        timing.complete();
        validate_candidate_count(candidates, count)
    }

    async fn models(&self) -> Result<Vec<String>> {
        let request = self.client.get(format!("{}/models", self.base_url));
        let response = self.authenticated(request).send().await?;
        let response: OpenAiModels =
            decode_response(response, "Invalid OpenAI-compatible models response").await?;
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
struct OllamaStreamChunk {
    #[serde(default)]
    message: Option<OpenAiMessage>,
    #[serde(default)]
    done: bool,
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
    async fn generate(&self, prompt: &str, count: usize) -> Result<Vec<Candidate>> {
        let prompt = candidate_prompt(prompt, count);
        let body = OllamaRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: &prompt,
            }],
            stream: true,
            format: "json",
        };
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;
        let response = ensure_success(response).await?;
        let content = if is_streaming_content_type(&response) {
            consume_ndjson(response)
                .await
                .context("Failed to read streaming response; provider may not support streaming")?
        } else {
            // Ollama servers that ignore `stream: true` return one JSON body.
            let response: OllamaResponse =
                response.json().await.context("Invalid Ollama response")?;
            response.message.content
        };
        let candidates = parse_candidates(&content)?;
        validate_candidate_count(candidates, count)
    }

    async fn generate_traced(
        &self,
        prompt: &str,
        count: usize,
        timing: &mut GenerationTiming,
    ) -> Result<Vec<Candidate>> {
        let prompt = candidate_prompt(prompt, count);
        let body = OllamaRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user",
                content: &prompt,
            }],
            stream: true,
            format: "json",
        };
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;
        timing.mark_headers();
        let response = ensure_success(response).await?;
        let content = if is_streaming_content_type(&response) {
            consume_ndjson_traced(response, timing)
                .await
                .context("Failed to read streaming response; provider may not support streaming")?
        } else {
            let response: OllamaResponse =
                response.json().await.context("Invalid Ollama response")?;
            timing.mark_first_content();
            response.message.content
        };
        let candidates = parse_candidates(&content)?;
        timing.complete();
        validate_candidate_count(candidates, count)
    }

    async fn models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?;
        let response: OllamaModels =
            decode_response(response, "Invalid Ollama models response").await?;
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

fn candidate_prompt(prompt: &str, count: usize) -> String {
    format!("{prompt}\nReturn exactly {count} candidates in the candidates array.\n")
}

async fn decode_response<T: DeserializeOwned>(
    response: reqwest::Response,
    invalid_context: &'static str,
) -> Result<T> {
    let response = ensure_success(response).await?;
    response.json().await.context(invalid_context)
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = provider_error_detail(&body);
        anyhow::bail!("Provider returned {status}: {detail}");
    }
    Ok(response)
}

fn is_streaming_content_type(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| {
            content_type.contains("text/event-stream")
                || content_type.contains("application/x-ndjson")
        })
}

/// Accumulates `data:` payloads of a server-sent event stream until `[DONE]`
/// or end of stream.
async fn consume_sse(response: reqwest::Response) -> Result<String> {
    consume_sse_inner(response, None).await
}

async fn consume_sse_traced(
    response: reqwest::Response,
    timing: &mut GenerationTiming,
) -> Result<String> {
    consume_sse_inner(response, Some(timing)).await
}

async fn consume_sse_inner(
    response: reqwest::Response,
    mut timing: Option<&mut GenerationTiming>,
) -> Result<String> {
    let mut content = String::new();
    let mut buffer = SseLineBuffer::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Stream interrupted")?;
        for line in buffer.push(&chunk) {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                return Ok(content);
            }
            if let Ok(chunk) = serde_json::from_str::<OpenAiStreamChunk>(data) {
                if let Some(delta) = chunk
                    .choices
                    .first()
                    .and_then(|choice| choice.delta.content.as_deref())
                {
                    if !delta.is_empty() {
                        if let Some(timing) = timing.as_deref_mut() {
                            timing.mark_first_content();
                        }
                    }
                    content.push_str(delta);
                }
            }
        }
    }
    anyhow::ensure!(!content.is_empty(), "Provider returned an empty stream");
    Ok(content)
}

/// Accumulates `message.content` of an Ollama NDJSON stream until `done` or
/// end of stream.
async fn consume_ndjson(response: reqwest::Response) -> Result<String> {
    consume_ndjson_inner(response, None).await
}

async fn consume_ndjson_traced(
    response: reqwest::Response,
    timing: &mut GenerationTiming,
) -> Result<String> {
    consume_ndjson_inner(response, Some(timing)).await
}

async fn consume_ndjson_inner(
    response: reqwest::Response,
    mut timing: Option<&mut GenerationTiming>,
) -> Result<String> {
    let mut content = String::new();
    let mut buffer = SseLineBuffer::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Stream interrupted")?;
        for line in buffer.push(&chunk) {
            if let Ok(chunk) = serde_json::from_str::<OllamaStreamChunk>(line.trim()) {
                if let Some(message) = chunk.message.as_ref() {
                    if !message.content.is_empty() {
                        if let Some(timing) = timing.as_deref_mut() {
                            timing.mark_first_content();
                        }
                    }
                    content.push_str(&message.content);
                }
                if chunk.done {
                    return Ok(content);
                }
            }
        }
    }
    anyhow::ensure!(!content.is_empty(), "Provider returned an empty stream");
    Ok(content)
}

/// Splits an arbitrary byte stream into complete lines, keeping a partial
/// trailing line for the next push. Empty lines are dropped.
#[derive(Default)]
struct SseLineBuffer {
    carry: String,
}

impl SseLineBuffer {
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.carry
            .push_str(std::str::from_utf8(chunk).unwrap_or_default());
        let mut lines = Vec::new();
        let mut rest = self.carry.as_str();
        while let Some(newline) = rest.find('\n') {
            let line = rest[..newline].trim_end_matches('\r');
            if !line.is_empty() {
                lines.push(line.to_owned());
            }
            rest = &rest[newline + 1..];
        }
        self.carry = rest.to_owned();
        lines
    }
}

fn provider_error_detail(body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let detail = parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
        })
        .and_then(serde_json::Value::as_str)
        .unwrap_or(body)
        .trim();
    let cleaned: String = detail
        .chars()
        .filter(|character| !character.is_control())
        .take(500)
        .collect();
    if cleaned.is_empty() {
        "No error details returned".into()
    } else {
        cleaned
    }
}

pub(crate) fn validate_endpoint(base_url: &str) -> Result<()> {
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

    #[test]
    fn line_buffer_splits_lines_across_chunk_boundaries() {
        let mut buffer = SseLineBuffer::default();
        assert_eq!(buffer.push(b"data: {\"a\":1}\nda"), vec!["data: {\"a\":1}"]);
        assert_eq!(buffer.push(b"ta: [DONE]\n\n"), vec!["data: [DONE]"]);
    }

    #[test]
    fn line_buffer_tolerates_crlf_and_empty_lines() {
        let mut buffer = SseLineBuffer::default();
        assert_eq!(
            buffer.push(b"data: one\r\ndata: two\r\n\r\n"),
            vec!["data: one", "data: two"]
        );
        assert_eq!(buffer.push(b"data: three\r\n"), vec!["data: three"]);
    }
}
