# Contributing

Thank you for improving commit-wisp.

1. Open an issue for behavior changes so the user-facing contract is clear.
2. Add or update a test first and run it to establish an intentional RED result.
3. Implement the smallest change that makes the test GREEN.
4. Run formatting, Clippy, all tests, and a release build.
5. Keep commits focused and use Conventional Commits.

Required checks:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
```

Tests that contact a real provider or use a real API key are not accepted. Use a loopback mock server. New secret-detection rules must assert that debug/error output does not contain the matched value.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
