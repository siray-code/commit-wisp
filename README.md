<p align="center">
  <img src="assets/logo.svg" alt="commit-wisp AI Git commit message generator logo" width="640">
</p>

# commit-wisp — AI Git Commit Message Generator

`commit-wisp` is an AI commit message generator for Git that turns staged changes into reviewable [Conventional Commits](#does-commit-wisp-generate-conventional-commits). Use an OpenAI-compatible cloud provider or local Ollama, then review every candidate in a terminal UI before committing. Sensitive-data detection runs before any diff is sent, and token-aware compression keeps large diffs within the configured input budget.

[简体中文](README.zh-CN.md)

## Core capabilities

- Generates commit message candidates from the Git index—never from unstaged files.
- Requires human selection or editing in an interactive terminal UI before committing.
- Supports local Ollama and OpenAI-compatible APIs such as OpenAI, OpenRouter, DeepSeek, Groq, and compatible gateways.
- Scans added diff lines for common credential patterns before making a provider request.
- Compresses large diffs locally with a visible, provider-independent token estimate.
- Calls native `git commit -F`, preserving normal Git hooks, signing, and repository behavior. `--no-verify` is opt-in.

## Quick start

### macOS

```sh
brew install siray-code/tap/commit-wisp
commit-wisp setup
git add src tests
commit-wisp
```

### Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.sh | sh
commit-wisp setup
git add src tests
commit-wisp
```

If `~/.local/bin` is not on `PATH`, run `~/.local/bin/commit-wisp` or add that directory to `PATH`.

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
commit-wisp setup
git add src tests
commit-wisp
```

The Windows installer places `commit-wisp.exe` in `%LOCALAPPDATA%\Programs\commit-wisp\bin` and automatically adds that directory to the user `PATH`.

Select a candidate in the terminal UI, edit it if needed, and press `Enter` to commit. To generate candidates without committing:

```sh
commit-wisp --dry-run
```

On Windows, use `commit-wisp --dry-run`.

## How it works

1. **Read the Git index.** `commit-wisp` runs `git diff --cached` with external diff and text conversion disabled. An empty index stops the run.
2. **Scan before sending.** Only added diff lines are checked for common secret patterns. If a match is found, the provider request is blocked unless you explicitly pass `--allow-sensitive`.
3. **Compress the staged diff locally.** Lockfiles, minified files, and generated content are summarized; remaining files share the token budget so one large file cannot hide the rest.
4. **Generate candidates.** The compressed diff, diff statistics, and recent commit subjects are sent to the configured Ollama or OpenAI-compatible provider.
5. **Review in the TUI.** Choose, edit, regenerate, copy, change model, or cancel. Cancellation does not modify the index.
6. **Commit through Git.** After confirmation, the selected message is written to a temporary file and passed to native `git commit -F`. Hooks and signing remain under Git's control.

## Capability overview

| Capability | Behavior |
| --- | --- |
| Input scope | Staged changes from `git diff --cached` |
| Message format | Conventional Commits by default; format and language are configurable |
| Providers | OpenAI-compatible APIs and local Ollama |
| Human review | Select, edit, regenerate, change model, copy, commit, or cancel in the TUI |
| Sensitive data | Added lines scanned locally; likely secrets block transmission by default |
| Large diffs | Deterministic local compression with before/after token estimates |
| Git behavior | Native `git commit -F`; hooks and signing preserved |
| Non-commit mode | `--dry-run`, or automatic candidate output when no interactive terminal is available |

## Installation

### Homebrew

```sh
brew install siray-code/tap/commit-wisp
```

### macOS and Linux installer

For macOS or glibc-based Linux, the installer defaults to `~/.local/bin` and verifies the release checksum:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.sh | sh
```

Set `COMMIT_WISP_VERSION` for a specific release or `COMMIT_WISP_INSTALL_DIR` for another destination.

### Windows PowerShell

The x64/arm64 installer downloads `commit-wisp.exe` into `%LOCALAPPDATA%\Programs\commit-wisp\bin`, verifies `SHA256SUMS`, and automatically adds that directory to the user `PATH`:

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
```

The updated `PATH` is available immediately in the installer session; newly opened terminals inherit it as well. Set `COMMIT_WISP_INSTALL_DIR` to install elsewhere. Release archives and checksums are also available on the [GitHub Releases page](https://github.com/siray-code/commit-wisp/releases).

### Build from source

Rust 1.88 or newer is required:

```sh
cargo install --git https://github.com/siray-code/commit-wisp --locked
```

Then run `commit-wisp setup` to configure a provider, model, endpoint, and credential storage.

## Terminal review and CLI examples

Inside the review UI:

- `↑`/`↓` or `j`/`k`: select a candidate
- `Enter`: commit with the selected message
- `e`: edit using Git's editor configuration, then the platform fallback
- `r`: regenerate candidates
- `m`: switch to the next discovered model and regenerate
- `c`: copy the selected message
- `q`: cancel without changing staged content

Useful commands:

```sh
commit-wisp --dry-run
commit-wisp --model qwen3 --prompt "Focus on compatibility impact"
commit-wisp doctor
commit-wisp completions zsh > _commit-wisp
```

`commit-wisp` never stages files or pushes commits. Passing `--no-verify` forwards that flag to Git only after you select a message.

## Providers and credentials

### OpenAI-compatible APIs

The default endpoint is `https://api.openai.com/v1`. OpenAI, OpenRouter, DeepSeek, Groq, and compatible gateways use the same provider interface.

During `commit-wisp setup`, credentials can be stored in the operating-system credential store (`system`) or in a separate credentials file (`file`). System storage uses Keychain, Credential Manager, or Secret Service. File storage is plaintext in `credentials.toml`, protected with user-only `0600` permissions on Unix, and is not shown by normal configuration commands.

For ephemeral or CI use, `COMMIT_WISP_API_KEY` takes priority over stored credentials:

```sh
export COMMIT_WISP_API_KEY='...'
export COMMIT_WISP_BASE_URL='https://api.example.com/v1'
commit-wisp --provider openai-compatible --model model-name --dry-run
```

To explicitly choose the system credential store:

```sh
commit-wisp setup --credential-store system
```

On macOS, select **Always Allow** for a stable installed binary if Keychain asks. Locally rebuilt ad-hoc binaries may be treated as different applications after each build.

### Local Ollama

Ollama does not require an API key:

```sh
ollama serve
commit-wisp config set provider ollama
commit-wisp config set base_url http://localhost:11434
commit-wisp config set model qwen3
commit-wisp --dry-run
```

Provider URLs must use HTTPS. Plain HTTP is accepted only for `localhost` or `127.0.0.1`.

## Configuration and prompt templates

Configuration precedence is: CLI flags → `COMMIT_WISP_*` environment variables → repository `.commit-wisp.toml` → global configuration → defaults. See the [example configuration](examples/commit-wisp.toml).

Prompt templates support `{{diff}}`, `{{stats}}`, `{{recent_commits}}`, `{{language}}`, `{{format}}`, `{{candidate_count}}`, and `{{extra_instruction}}`. A custom template must include `{{diff}}`; see the [example prompt](examples/prompt.txt).

```sh
commit-wisp prompt show
commit-wisp prompt init
commit-wisp prompt edit
commit-wisp prompt reset
```

`prompt init` creates a global `prompt.txt` by default. `--prompt "instruction"` adds a one-run instruction without replacing the configured template. The default template requests the configured number of Conventional Commit candidates: type and optional scope use lowercase English, while the summary and optional body use the configured language. Bodies are omitted when the subject is sufficient; otherwise they may contain only facts supported by staged changes. Recent commits influence style, not evidence rules.

Diff compression is deterministic and local. Lockfiles, minified files, and generated content are represented by filename and statistics. Token counts are provider-independent estimates rather than provider billing counts.

## Security model

The staged diff is untrusted and may contain credentials. `commit-wisp` scans added lines for common AWS keys, private keys, generic API keys, and GitHub token patterns. Findings report only the file, line, and rule name; the matched secret is not retained in the finding. A match blocks the provider request by default.

The scanner is a safety preflight, not a guarantee that every secret will be found. Inspect the staged diff instead of routinely using `--allow-sensitive`. Read [SECURITY.md](SECURITY.md) for reporting guidance and limitations.

## FAQ

### Does commit-wisp generate Conventional Commits?

Yes. The built-in prompt generates Conventional Commit candidates by default. You can change the configured `format`, language, model, or prompt template.

### Can I use commit-wisp locally with Ollama?

Yes. Set `provider` to `ollama`, use the local Ollama endpoint, and select an installed model. No diff needs to go to a cloud provider.

### Does it read unstaged files?

No. Message generation uses only `git diff --cached`—the Git index. Unstaged working-tree changes and untracked files are not included unless you stage them first.

### Does it commit automatically?

No in an interactive terminal. You must select a candidate in the TUI and press `Enter`. `--dry-run` prints candidates without committing, and non-interactive sessions also print candidates instead of committing.

### What happens if the staged diff contains a secret?

Likely credentials on added lines block the provider request before anything is sent. Review or unstage the content. `--allow-sensitive` is an explicit override, not a redaction feature, and the scanner cannot detect every secret format.

### Are Git hooks and commit signing preserved?

Yes. The final action uses native `git commit -F`, so configured hooks and signing behavior still apply. Hooks are skipped only when you explicitly pass `--no-verify`.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo llvm-cov --all-features --fail-under-lines 80
cargo build --release
```

Run tests without a local Rust installation:

```sh
docker run --rm -v "$PWD":/app -w /app rust:1.88 cargo test
```

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

MIT
