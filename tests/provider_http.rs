use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use commit_wisp::provider::{
    generate_candidates_parallel_traced, LlmProvider, OllamaProvider, OpenAiProvider,
};

fn mock_server(response: &'static str) -> (String, thread::JoinHandle<String>) {
    mock_server_with_status("200 OK", response)
}

fn mock_server_with_status(
    status: &'static str,
    response: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = vec![0_u8; 32_768];
        let count = stream.read(&mut request).expect("read request");
        let body = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        );
        stream.write_all(body.as_bytes()).expect("write response");
        String::from_utf8_lossy(&request[..count]).into_owned()
    });
    (format!("http://{address}"), handle)
}

fn mock_sse_server_with_status(
    status: &'static str,
    content_type: &'static str,
    body: String,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = vec![0_u8; 32_768];
        let count = stream.read(&mut request).expect("read request");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n{body}"
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        String::from_utf8_lossy(&request[..count]).into_owned()
    });
    (format!("http://{address}"), handle)
}

#[tokio::test]
async fn openai_provider_reports_structured_error_details() {
    let response = r#"{"error":{"message":"n must be 1","type":"invalid_request_error"}}"#;
    let (base_url, handle) = mock_server_with_status("400 Bad Request", response);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), None, 5).expect("provider");

    let error = provider
        .generate("test prompt", 3)
        .await
        .expect_err("provider error");
    assert!(error.to_string().contains("400 Bad Request: n must be 1"));
    handle.join().expect("server thread");
}

#[tokio::test]
async fn openai_provider_sends_authenticated_chat_request() {
    // Content-Type: application/json — the gateway ignored `stream: true`,
    // so this also exercises the non-streaming fallback parsing path.
    let response = r#"{"choices":[{"message":{"content":"{\"candidates\":[{\"subject\":\"feat: add provider\",\"body\":null},{\"subject\":\"feat(api): expose provider\",\"body\":null},{\"subject\":\"feat(config): configure provider\",\"body\":null}]}"}}]}"#;
    let (base_url, handle) = mock_server(response);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), Some("secret-key".into()), 5)
        .expect("provider");

    let candidates = provider
        .generate("test prompt", 3)
        .await
        .expect("generation");
    assert_eq!(candidates[0].subject, "feat: add provider");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer secret-key"));
    assert!(request.contains("test-model"));
    assert!(request.contains("Return exactly 3 candidates"));
    assert!(request.contains("\"stream\":true"));
    assert!(!request.contains("\"n\":"));
}

#[tokio::test]
async fn ollama_provider_uses_native_chat_protocol() {
    // Content-Type: application/json — the server ignored `stream: true`,
    // so this also exercises the non-streaming fallback parsing path.
    let response = r#"{"message":{"content":"{\"candidates\":[{\"subject\":\"fix: local model\",\"body\":null},{\"subject\":\"fix(ollama): use local model\",\"body\":null}]}"}}"#;
    let (base_url, handle) = mock_server(response);
    let provider = OllamaProvider::new(base_url, "qwen3".into(), 5).expect("provider");

    let candidates = provider
        .generate("test prompt", 2)
        .await
        .expect("generation");
    assert_eq!(candidates[0].subject, "fix: local model");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /api/chat HTTP/1.1"));
    assert!(request.contains("\"stream\":true"));
    assert!(request.contains("Return exactly 2 candidates"));
}

#[tokio::test]
async fn providers_reject_an_unexpected_candidate_count() {
    let response = r#"{"choices":[{"message":{"content":"{\"candidates\":[{\"subject\":\"feat: only one\",\"body\":null}]}"}}]}"#;
    let (base_url, handle) = mock_server(response);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), None, 5).expect("provider");

    let error = provider
        .generate("test prompt", 2)
        .await
        .expect_err("candidate count must be exact");
    assert!(error.to_string().contains("exactly 2 candidates"));
    handle.join().expect("server thread");
}

#[tokio::test]
async fn providers_discover_and_sort_models() {
    let (openai_url, openai_handle) =
        mock_server(r#"{"data":[{"id":"z-model"},{"id":"a-model"}]}"#);
    let openai = OpenAiProvider::new(openai_url, "test".into(), None, 5).expect("provider");
    assert_eq!(openai.models().await.unwrap(), vec!["a-model", "z-model"]);
    assert!(openai_handle.join().unwrap().starts_with("GET /models"));

    let (ollama_url, ollama_handle) =
        mock_server(r#"{"models":[{"name":"qwen"},{"name":"llama"}]}"#);
    let ollama = OllamaProvider::new(ollama_url, "test".into(), 5).expect("provider");
    assert_eq!(ollama.models().await.unwrap(), vec!["llama", "qwen"]);
    assert!(ollama_handle.join().unwrap().starts_with("GET /api/tags"));
}

#[test]
fn rejects_plain_http_for_non_local_provider_endpoints() {
    assert!(OpenAiProvider::new("http://example.com/v1".into(), "model".into(), None, 5,).is_err());
    assert!(OllamaProvider::new("http://example.com".into(), "model".into(), 5).is_err());
}

#[tokio::test]
async fn traced_openai_stream_records_headers_first_content_and_completion() {
    let candidates = r#"{"candidates":[{"subject":"feat: trace stream","body":null}]}"#;
    let content = serde_json::to_string(candidates).expect("encode content");
    let body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{content}}}}}]}}\n\ndata: [DONE]\n\n"
    );
    let (base_url, handle) = mock_sse_server_with_status("200 OK", "text/event-stream", body);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), None, 5).expect("provider");

    let (_, timings) = generate_candidates_parallel_traced(&provider, "prompt", 1)
        .await
        .expect("traced generation");

    assert!(timings[0].response_headers.is_some());
    assert!(timings[0].first_content.is_some());
    assert!(timings[0].completed.is_some());
    handle.join().expect("server thread");
}

#[tokio::test]
async fn openai_provider_streams_sse_and_accumulates_candidates() {
    let candidates = r#"{"candidates":[{"subject":"feat: stream","body":null},{"subject":"feat(api): stream","body":null},{"subject":"feat(config): stream","body":null}]}"#;
    let (raw_head, raw_tail) = candidates.split_at(candidates.len() / 2);
    let head = serde_json::to_string(raw_head).expect("encode head");
    let tail = serde_json::to_string(raw_tail).expect("encode tail");
    let body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{\"content\":{head}}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{\"content\":{tail}}}}}]}}\n\ndata: [DONE]\n\n"
    );
    let (base_url, handle) = mock_sse_server_with_status("200 OK", "text/event-stream", body);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), None, 5).expect("provider");

    let generated = provider
        .generate("test prompt", 3)
        .await
        .expect("streaming generation");
    assert_eq!(generated.len(), 3);
    assert_eq!(generated[0].subject, "feat: stream");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
    assert!(request.contains("\"stream\":true"));
}

#[tokio::test]
async fn openai_stream_error_status_surfaces_before_parsing() {
    let body = r#"{"error":{"message":"bad gateway","type":"upstream"}}"#.to_owned();
    let (base_url, handle) =
        mock_sse_server_with_status("502 Bad Gateway", "text/event-stream", body);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), None, 5).expect("provider");

    let error = provider
        .generate("test prompt", 2)
        .await
        .expect_err("provider error");
    assert!(error.to_string().contains("Provider returned 502"));
    handle.join().expect("server thread");
}

#[tokio::test]
async fn ollama_provider_streams_ndjson_and_accumulates_candidates() {
    let candidates = r#"{"candidates":[{"subject":"fix: local stream","body":null},{"subject":"fix(ollama): local stream","body":null}]}"#;
    let (raw_head, raw_tail) = candidates.split_at(candidates.len() / 2);
    let head = serde_json::to_string(raw_head).expect("encode head");
    let tail = serde_json::to_string(raw_tail).expect("encode tail");
    let body = format!(
        "{{\"message\":{{\"role\":\"assistant\",\"content\":{head}}},\"done\":false}}\n{{\"message\":{{\"role\":\"assistant\",\"content\":{tail}}},\"done\":false}}\n{{\"message\":{{\"role\":\"assistant\",\"content\":\"\"}},\"done\":true}}\n"
    );
    let (base_url, handle) = mock_sse_server_with_status("200 OK", "application/x-ndjson", body);
    let provider = OllamaProvider::new(base_url, "qwen3".into(), 5).expect("provider");

    let generated = provider
        .generate("test prompt", 2)
        .await
        .expect("streaming generation");
    assert_eq!(generated.len(), 2);
    assert_eq!(generated[0].subject, "fix: local stream");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /api/chat HTTP/1.1"));
    assert!(request.contains("\"stream\":true"));
}
