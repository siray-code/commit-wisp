# Security Policy

## Reporting

Do not open a public issue for a vulnerability or include real credentials, proprietary diffs, or provider responses in a report. Once a remote repository exists, use its private security-advisory feature or contact the maintainer privately.

## Credential and diff handling

- API keys are read from `COMMIT_WISP_API_KEY` or the operating-system credential store.
- TOML configuration does not contain a key field.
- Added diff lines are scanned before provider creation and network access.
- Findings contain only filename, diff line, and rule identifier.
- `--allow-sensitive` is an explicit escape hatch and sends the resulting payload unchanged.
- Custom providers receive source content. Review their privacy and retention policy.

Secret detection is defense in depth, not a guarantee. It can miss encoded, split, novel, or domain-specific secrets. Always inspect staged changes and use Ollama when source code must remain local.

Supported releases will be listed after the first tagged release. Until then, security fixes target the `main` branch.
