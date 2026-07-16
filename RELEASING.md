# Releasing commit-wisp

Pushing a semantic-version tag builds GitHub-hosted Release assets for Linux, macOS, and Windows, publishes their SHA-256 checksums, and updates the Homebrew tap.

## One-time setup

1. Create the `siray-code/homebrew-tap` repository with a `Formula` directory.
2. Create a fine-grained GitHub token with repository contents write access to that tap.
3. Add it to this repository as the Actions secret `HOMEBREW_TAP_TOKEN`.

Without the secret, GitHub Releases still publish successfully and the Homebrew job reports that tap publishing was skipped.

## Publish a version

Keep `Cargo.toml` and `Cargo.lock` on the version being released, then run:

```sh
cargo test --all-features
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin v0.1.0
```

The release workflow:

1. builds native archives for x86-64 and ARM64 Linux/macOS, plus x86-64 Windows;
2. publishes the archives, install scripts, generated release notes, and `SHA256SUMS`;
3. renders `Formula/commit-wisp.rb` with immutable release URLs and checksums;
4. commits the formula update to `siray-code/homebrew-tap`.

The tag version should match the package version. If any build fails, the release and Homebrew update do not run.
