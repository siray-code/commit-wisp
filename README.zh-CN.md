<p align="center">
  <img src="assets/logo.svg" alt="commit-wisp AI Git 提交信息生成器标志" width="640">
</p>

# commit-wisp — AI Git Commit Message Generator

`commit-wisp` 是一个面向 Git 的 AI commit message generator：它根据暂存区改动生成可审阅的 [Conventional Commits](#是否生成-conventional-commits)，支持 OpenAI-compatible 云端服务与本地 Ollama，并在提交前通过 terminal UI 让你确认或编辑。diff 发出前会执行敏感信息检测，大型 diff 则会按 token 预算在本地压缩。

[English](README.md)

## 核心能力

- 只根据 Git index（暂存区）生成候选，不读取 unstaged files。
- 在交互式终端中必须由人工选择或编辑后才能提交。
- 支持本地 Ollama，以及 OpenAI、OpenRouter、DeepSeek、Groq 等 OpenAI-compatible API。
- 发起 Provider 请求前，扫描新增 diff 行中的常见凭据模式。
- 在本地对大型 diff 做 token-aware 压缩，并展示 Provider 无关的 token 估算。
- 通过原生 `git commit -F` 提交，保留 Git hooks、签名与仓库行为；`--no-verify` 仅在显式传入时生效。

## 快速开始

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

如果 `~/.local/bin` 不在 `PATH` 中，请直接运行 `~/.local/bin/commit-wisp`，或将该目录加入 `PATH`。

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
commit-wisp setup
git add src tests
commit-wisp
```

Windows 安装脚本会把 `commit-wisp.exe` 放到 `%LOCALAPPDATA%\Programs\commit-wisp\bin`，并自动把该目录加入用户 `PATH`。

在 terminal UI 中选择候选，必要时编辑，然后按 `Enter` 提交。只生成候选、不提交：

```sh
commit-wisp --dry-run
```

Windows 中请使用 `commit-wisp --dry-run`。

## 工作原理

1. **读取 Git index。** 运行禁用 external diff 与 text conversion 的 `git diff --cached`；暂存区为空时立即停止。
2. **发送前扫描。** 只检查新增 diff 行中的常见密钥模式。发现匹配时默认阻止 Provider 请求，除非显式使用 `--allow-sensitive`。
3. **本地压缩 staged diff。** lockfile、minified file 和生成内容会被摘要；其余文件共享 token 预算，避免单个大文件挤掉其他改动。
4. **生成候选。** 将压缩后的 diff、diff 统计与近期 commit subject 发送给配置的 Ollama 或 OpenAI-compatible Provider。
5. **在 TUI 审阅。** 可以选择、编辑、重新生成、复制、切换模型或取消；取消不会修改暂存区。
6. **通过 Git 提交。** 确认后把消息写入临时文件，再调用原生 `git commit -F`；hooks 和签名仍由 Git 控制。

## 能力一览

| 能力 | 实际行为 |
| --- | --- |
| 输入范围 | `git diff --cached` 中的暂存改动 |
| 消息格式 | 默认 Conventional Commits；格式和语言可配置 |
| Providers | OpenAI-compatible API 与本地 Ollama |
| 人工审阅 | 在 TUI 中选择、编辑、重生成、切换模型、复制、提交或取消 |
| 敏感信息 | 本地扫描新增行；疑似密钥默认阻止发送 |
| 大型 diff | 确定性的本地压缩，并展示压缩前后 token 估算 |
| Git 行为 | 原生 `git commit -F`；保留 hooks 与签名 |
| 不提交模式 | `--dry-run`；没有交互式终端时也只输出候选 |

## 安装

### Homebrew

```sh
brew install siray-code/tap/commit-wisp
```

### macOS 与 Linux 安装脚本

适用于 macOS 或基于 glibc 的 Linux，默认安装到 `~/.local/bin`，并校验 release checksum：

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.sh | sh
```

使用 `COMMIT_WISP_VERSION` 可指定版本，使用 `COMMIT_WISP_INSTALL_DIR` 可指定安装目录。

### Windows PowerShell

x64/arm64 脚本将 `commit-wisp.exe` 下载到 `%LOCALAPPDATA%\Programs\commit-wisp\bin`，校验 `SHA256SUMS`，并自动把该目录加入用户 `PATH`：

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
```

安装脚本所在的 PowerShell 会立即获得更新后的 `PATH`，新打开的终端也会继承该配置。可通过 `COMMIT_WISP_INSTALL_DIR` 指定其他安装目录。也可从 [GitHub Releases](https://github.com/siray-code/commit-wisp/releases) 手动下载并核对 checksum。

### 从源码构建

需要 Rust 1.88 或更新版本：

```sh
cargo install --git https://github.com/siray-code/commit-wisp --locked
```

安装后运行 `commit-wisp setup`，配置 Provider、模型、endpoint 和凭据存储方式。

## 终端审阅与 CLI 示例

review UI 快捷键：

- `↑`/`↓` 或 `j`/`k`：选择候选
- `Enter`：使用所选消息提交
- `e`：使用 Git editor 配置或平台 fallback 编辑
- `r`：重新生成
- `m`：切换到下一个发现的模型并重新生成
- `c`：复制所选消息
- `q`：取消且不改变暂存内容

常用命令：

```sh
commit-wisp --dry-run
commit-wisp --model qwen3 --prompt "重点说明兼容性影响"
commit-wisp doctor
commit-wisp completions zsh > _commit-wisp
```

`commit-wisp` 不会自动 stage 文件，也不会 push。`--no-verify` 只会在你选择消息后转交给 Git。

## Providers 与凭据

### OpenAI-compatible API

默认 endpoint 是 `https://api.openai.com/v1`。OpenAI、OpenRouter、DeepSeek、Groq 与兼容 gateway 共用同一 Provider 接口。

运行 `commit-wisp setup` 时，可以选择操作系统凭据库（`system`）或独立凭据文件（`file`）。system 模式使用 Keychain、Credential Manager 或 Secret Service；file 模式在 `credentials.toml` 中明文保存，Unix 上限制为当前用户可读写的 `0600` 权限，普通配置命令不会显示它。

临时环境或 CI 可使用优先级更高的环境变量：

```sh
export COMMIT_WISP_API_KEY='...'
export COMMIT_WISP_BASE_URL='https://api.example.com/v1'
commit-wisp --provider openai-compatible --model model-name --dry-run
```

显式选择系统凭据库：

```sh
commit-wisp setup --credential-store system
```

macOS Keychain 对稳定安装的二进制询问权限时可选 **Always Allow**。频繁本地重建的临时签名二进制可能被系统视为不同应用。

### 本地 Ollama

Ollama 不需要 API Key：

```sh
ollama serve
commit-wisp config set provider ollama
commit-wisp config set base_url http://localhost:11434
commit-wisp config set model qwen3
commit-wisp --dry-run
```

Provider URL 必须使用 HTTPS；只有 `localhost` 或 `127.0.0.1` 可以使用明文 HTTP。

## 配置与 Prompt 模板

配置优先级为：CLI 参数 → `COMMIT_WISP_*` 环境变量 → 仓库 `.commit-wisp.toml` → 全局配置 → 默认值。参见[配置示例](examples/commit-wisp.toml)。

Prompt 模板支持 `{{diff}}`、`{{stats}}`、`{{recent_commits}}`、`{{language}}`、`{{format}}`、`{{candidate_count}}` 和 `{{extra_instruction}}`。自定义模板必须包含 `{{diff}}`；参见 [Prompt 示例](examples/prompt.txt)。

```sh
commit-wisp prompt show
commit-wisp prompt init
commit-wisp prompt edit
commit-wisp prompt reset
```

`prompt init` 默认创建全局 `prompt.txt`。`--prompt "要求"` 只为本次运行追加指令，不会替换已配置模板。默认模板按配置数量生成 Conventional Commit 候选：type 和可选 scope 使用英文小写，摘要与可选正文使用配置语言。subject 足以说明改动时省略正文；否则正文也只能包含暂存区能够证明的事实。近期提交只影响风格，不改变证据约束。

diff 压缩是确定性的本地操作。lockfile、minified file 和生成内容以文件名与统计信息表示。token 数量是 Provider 无关的估算，并非 Provider 计费值。

## 安全模型

暂存 diff 属于不可信输入，可能包含凭据。`commit-wisp` 会扫描新增行中的常见 AWS key、private key、通用 API key 与 GitHub token 模式。匹配结果只报告文件、行号与规则名称，不把匹配到的 secret 保存在 finding 中；发现匹配时默认阻止 Provider 请求。

该扫描只是安全预检，不能保证发现所有 secret。应检查 staged diff，不要习惯性使用 `--allow-sensitive`。安全报告方式与限制参见 [SECURITY.md](SECURITY.md)。

## FAQ

### 是否生成 Conventional Commits？

是。内置 Prompt 默认生成 Conventional Commit 候选，也可以修改 `format`、语言、模型或 Prompt 模板。

### 能否完全在本地配合 Ollama 使用？

可以。把 `provider` 设为 `ollama`，使用本地 Ollama endpoint 并选择已安装模型，diff 就不需要发送到云端 Provider。

### 是否读取 unstaged files？

不会。生成消息时只使用 Git index 中的 `git diff --cached`。未暂存改动和 untracked files 不会被包含，除非先执行 `git add`。

### 是否会自动提交？

在交互式终端中不会。必须在 TUI 选择候选并按 `Enter`。`--dry-run` 只打印候选；非交互式会话同样只打印，不提交。

### staged diff 包含 secret 时会怎样？

新增行中的疑似凭据会在发送前阻止 Provider 请求。请检查、修改或取消暂存相关内容。`--allow-sensitive` 是显式绕过，不是脱敏功能；扫描器也无法识别所有 secret 格式。

### 是否保留 Git hooks 与 commit signing？

是。最终使用原生 `git commit -F`，因此配置的 hooks 与 signing 行为仍然有效。只有显式传入 `--no-verify` 才会跳过 hooks。

## 开发

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo llvm-cov --all-features --fail-under-lines 80
cargo build --release
```

没有本地 Rust 环境时也可运行测试：

```sh
docker run --rm -v "$PWD":/app -w /app rust:1.88 cargo test
```

欢迎贡献。请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [Code of Conduct](CODE_OF_CONDUCT.md)。

## 许可证

MIT
