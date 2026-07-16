use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use commit_wisp::provider::{LlmProvider, OllamaProvider, OpenAiProvider};

fn mock_server(response: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let address = listener.local_addr().expect("mock address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = vec![0_u8; 32_768];
        let count = stream.read(&mut request).expect("read request");
        let body = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(), response
        );
        stream.write_all(body.as_bytes()).expect("write response");
        String::from_utf8_lossy(&request[..count]).into_owned()
    });
    (format!("http://{address}"), handle)
}

#[tokio::test]
async fn openai_provider_sends_authenticated_chat_request() {
    let response = r#"{"choices":[{"message":{"content":"{\"candidates\":[{\"subject\":\"feat: add provider\"}]}"}}]}"#;
    let (base_url, handle) = mock_server(response);
    let provider = OpenAiProvider::new(base_url, "test-model".into(), Some("secret-key".into()), 5)
        .expect("provider");

    let candidates = provider
        .generate("test prompt", 1)
        .await
        .expect("generation");
    assert_eq!(candidates[0].subject, "feat: add provider");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer secret-key"));
    assert!(request.contains("test-model"));
}

#[tokio::test]
async fn ollama_provider_uses_native_chat_protocol() {
    let response =
        r#"{"message":{"content":"{\"candidates\":[{\"subject\":\"fix: local model\"}]}"}}"#;
    let (base_url, handle) = mock_server(response);
    let provider = OllamaProvider::new(base_url, "qwen3".into(), 5).expect("provider");

    let candidates = provider
        .generate("test prompt", 1)
        .await
        .expect("generation");
    assert_eq!(candidates[0].subject, "fix: local model");
    let request = handle.join().expect("server thread");
    assert!(request.starts_with("POST /api/chat HTTP/1.1"));
    assert!(request.contains("\"stream\":false"));
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
