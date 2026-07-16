# commit-wisp

一个可审阅、可控制 token 预算的 AI Git commit CLI。

`commit-wisp` 只读取 Git 暂存区，在联网前检查疑似密钥并压缩大型 diff，然后在全屏终端界面中展示多个候选提交信息。只有你确认后，它才会调用原生 `git commit`。

[English](README.md)

## 特性

- 默认英文 Conventional Commits，可配置语言、格式、模型和 Prompt。
- 支持 OpenAI-compatible API 与本地 Ollama。
- API Key 保存到系统凭据库，配置和日志不输出密钥。
- 展示压缩前后 token 估算、被省略行数和暂存文件统计。
- 疑似凭据默认阻止发送；必须显式使用 `--allow-sensitive`。
- 交互界面支持选择、编辑、重生成、切换模型、复制、提交和取消。
- 不自动 `git add`、不 push；通过原生 Git 保留 hooks 与签名行为。

## 安装与使用

Homebrew：

```sh
brew install siray-code/tap/commit-wisp
```

macOS / 基于 glibc 的 Linux 一键安装（默认安装到 `~/.local/bin`，安装前校验 SHA-256）：

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.sh | sh
```

Windows PowerShell 一键安装（安装到当前用户目录并更新用户 `PATH`）：

```powershell
irm https://raw.githubusercontent.com/siray-code/commit-wisp/main/scripts/install.ps1 | iex
```

可通过 `COMMIT_WISP_VERSION` 指定版本，通过 `COMMIT_WISP_INSTALL_DIR` 指定安装目录。也可以从 [Releases](https://github.com/siray-code/commit-wisp/releases) 手动下载并核对 `SHA256SUMS`。

从源码构建需要 Rust 1.88 或更新版本：

```sh
cargo install --git https://github.com/siray-code/commit-wisp --locked
```

完成安装后：

```sh
commit-wisp setup

git add src tests
commit-wisp
```

只生成、不提交：

```sh
commit-wisp --dry-run
```

项目级配置可写入 `.commit-wisp.toml`；全局配置可通过 `commit-wisp config list|get|set` 管理。优先级为：命令行 > 环境变量 > 项目配置 > 全局配置 > 默认值。

详细配置、Prompt 变量、安全模型和贡献方式请参阅英文 [README](README.md)、[SECURITY.md](SECURITY.md) 与 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证

MIT
