<p align="center">
  <img src="assets/logo.svg" alt="commit-wisp — a glowing wisp following a Git commit trail" width="640">
</p>

<p align="center">
  Reviewable, token-aware AI commit messages from your staged Git changes.
</p>

# commit-wisp

Reviewable, token-aware AI commit messages from your staged Git changes.

`commit-wisp` reads only the Git index, blocks likely secrets before network access, compresses large diffs to a visible token budget, and lets you review or edit generated messages in a full-screen terminal UI before it calls `git commit`.

[简体中文](README.zh-CN.md)

## Why another AI commit tool?

- Review is mandatory in an interactive terminal; cancellation never changes the index.
- The exact staged payload is compressed locally with before/after token estimates.
- Likely credentials block provider requests unless `--allow-sensitive` is explicit.
- API keys live in the operating-system credential store, not TOML files.
- OpenAI-compatible endpoints and Ollama share one small, extensible provider boundary.
- Native `git commit -F` preserves hooks, signing, and repository behavior.

## Install

### Homebrew

```sh
brew install siray-code/tap/commit-wisp
```

### One-line installer

macOS and glibc-based Linux (installs to `~/.local/bin` and verifies the release checksum):

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.sh | sh
```

Windows PowerShell x64 and arm64 (downloads `commit-wisp.exe` into the current directory without changing `PATH`):

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
```

The Windows installer supports x64 and arm64. Run it from the directory where you want `commit-wisp.exe`, then use `.\commit-wisp.exe`. It verifies `SHA256SUMS` and never changes `PATH` or other environment variables.

Set `COMMIT_WISP_VERSION` to install a specific release, or `COMMIT_WISP_INSTALL_DIR` to choose a destination. Release archives and checksums are also available on the [Releases](https://github.com/siray-code/commit-wisp/releases) page.

### Build from source

Rust 1.88 or newer is required:

```sh
cargo install --git https://github.com/siray-code/commit-wisp --locked
```

Then configure a cloud-compatible endpoint or local Ollama:

```sh
commit-wisp setup
```

## Use

```sh
git add src tests
commit-wisp
```

Inside the review UI:

- `↑`/`↓` or `j`/`k`: select a candidate
- `Enter`: create the commit with the selected message
- `e`: edit with `$GIT_EDITOR` or `$EDITOR`
- `r`: regenerate
- `m`: switch to the next discovered model and regenerate
- `c`: copy the selected message
- `q`: cancel without touching staged changes

Useful non-interactive examples:

```sh
commit-wisp --dry-run
commit-wisp --model qwen3 --prompt "Focus on compatibility impact"
commit-wisp doctor
commit-wisp completions zsh > _commit-wisp
```

`--no-verify` is passed to Git only after explicit review. `commit-wisp` never stages or pushes files.

## Providers

### OpenAI-compatible

This covers OpenAI, OpenRouter, DeepSeek, Groq, and compatible gateways. The default endpoint is `https://api.openai.com/v1`. By default, `setup` stores the key in Keychain, Credential Manager, or Secret Service. On macOS, choose **Always Allow** for a stable installed binary. Locally rebuilt ad-hoc binaries may be treated as a new application after each build.

To avoid system credential prompts, explicitly select the file store during `setup` or run:

```sh
commit-wisp setup --credential-store file
```

File-store credentials are plaintext in a separate `credentials.toml`, protected with user-only (`0600`) permissions on Unix. They never appear in regular configuration output. For ephemeral/CI use:

```sh
export COMMIT_WISP_API_KEY='...'
export COMMIT_WISP_BASE_URL='https://api.example.com/v1'
commit-wisp --provider openai-compatible --model model-name --dry-run
```

### Ollama

```sh
ollama serve
commit-wisp config set provider ollama
commit-wisp config set base_url http://localhost:11434
commit-wisp config set model qwen3
```

Plain HTTP provider URLs are rejected unless they target `localhost` or `127.0.0.1`.

## Configuration and prompts

Precedence is CLI > `COMMIT_WISP_*` environment > repository `.commit-wisp.toml` > global configuration > defaults. See [`examples/commit-wisp.toml`](examples/commit-wisp.toml).

Project prompt templates can use `{{diff}}`, `{{stats}}`, `{{recent_commits}}`, `{{language}}`, `{{format}}`, `{{candidate_count}}`, and `{{extra_instruction}}`. A custom template must include `{{diff}}`; see [`examples/prompt.txt`](examples/prompt.txt). The default template produces the configured number of Conventional Commit candidates. Type and optional scope use lowercase English, while the summary and optional body use `{{language}}`. Bodies are omitted when the subject is sufficient and otherwise contain only facts supported by the staged changes. Recent commits influence style only and cannot override the output or evidence rules.

Prompt templates are directly manageable from the CLI:

```sh
commit-wisp prompt show
commit-wisp prompt init
commit-wisp prompt edit
commit-wisp prompt reset
```

`prompt init` creates a global `prompt.txt` by default. `--prompt "instruction"` remains a one-run addition and does not replace the configured template.
`prompt edit` follows Git's editor configuration and falls back to the system editor when none is configured.

Diff compression is deterministic and local. Lockfiles, minified files, and generated content are represented by filename/statistics; remaining files are independently budgeted so one large file cannot hide the rest. Token counts are provider-independent estimates.

## Security model

The staged diff is untrusted and may contain credentials. `commit-wisp` scans only added lines for common key formats, reports file/rule/line without retaining the matched value, and blocks transmission by default. Review the diff rather than habitually using `--allow-sensitive`.

See [SECURITY.md](SECURITY.md) for reporting and limitations.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo llvm-cov --all-features --fail-under-lines 80
cargo build --release
```

No local Rust installation is required for the same checks:

```sh
docker run --rm -v "$PWD":/app -w /app rust:1.88 cargo test
```

Contributions are welcome; read [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

MIT
