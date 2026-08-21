use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use commit_wisp::provider::{
    generate_candidates_parallel, generate_candidates_parallel_traced, Candidate, LlmProvider,
};

struct TrackingProvider {
    active: AtomicUsize,
    max_active: AtomicUsize,
    sequence: AtomicUsize,
}

impl TrackingProvider {
    fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            sequence: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for TrackingProvider {
    async fn generate(&self, _prompt: &str, count: usize) -> Result<Vec<Candidate>> {
        assert_eq!(
            count, 1,
            "each parallel request should ask for one candidate"
        );
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(25)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        let index = self.sequence.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Candidate {
            subject: format!("feat: candidate {index}"),
            body: None,
        }])
    }

    async fn models(&self) -> Result<Vec<String>> {
        Ok(vec!["test-model".into()])
    }

    fn model(&self) -> &str {
        "test-model"
    }
}

#[tokio::test]
async fn generates_multiple_candidates_with_concurrent_requests() {
    let provider = Arc::new(TrackingProvider::new());

    let candidates = generate_candidates_parallel(provider.as_ref(), "prompt", 3)
        .await
        .expect("parallel generation");

    assert_eq!(candidates.len(), 3);
    assert!(provider.max_active.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn reports_a_timing_for_each_parallel_request() {
    let provider = TrackingProvider::new();

    let (candidates, timings) = generate_candidates_parallel_traced(&provider, "prompt", 2)
        .await
        .expect("traced parallel generation");

    assert_eq!(candidates.len(), 2);
    assert_eq!(timings.len(), 2);
    assert_eq!(timings[0].request_index, 1);
    assert_eq!(timings[1].request_index, 2);
    assert!(timings.iter().all(|timing| timing.completed.is_some()));
}
