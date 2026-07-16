#[tokio::main]
async fn main() {
    if let Err(error) = commit_wisp::app::run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
