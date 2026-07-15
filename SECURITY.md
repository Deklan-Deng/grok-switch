# Security

Grok Switch is a **local desktop utility**. It does not run a cloud backend and does not phone home.

## What is stored where

| Data | Location (macOS) | Notes |
|------|------------------|--------|
| Provider list / settings | `~/Library/Application Support/GrokTokenSwitcher/profiles.json` | No API keys in normal flow |
| API tokens | `~/Library/Application Support/GrokTokenSwitcher/tokens.json` | Local file, restricted permissions when written |
| Active Grok CLI config | `~/.grok/config.toml` | Written **only when you enable** a provider |
| Config backups | next to `config.toml` (`.grok-switch-backup` suffix) | Created before overwrite |

Legacy Keychain entries (if any from older builds) may be migrated once into the local token vault.

## Network behavior

- **Enable / import / edit**: local filesystem only (plus optional `grok` CLI on your machine).
- **Health check / speed test**: HTTPS requests to the `base_url` you configured (e.g. `https://api.x.ai/v1` or a third-party compatible endpoint). Your API key is sent as `Authorization: Bearer …` to that host only.
- No analytics, crash telemetry, or automatic update phone-home is implemented in this project.

## What you should never commit

Do not put any of the following in git or screenshots of public issues:

- Real API keys / tokens
- Your personal `~/.grok/config.toml`
- `tokens.json` / `profiles.json` from Application Support
- Private relay hostnames you do not want public (optional; keys matter more)

## Reporting a vulnerability

If you find a security issue (for example token leakage, path traversal, or unsafe shell use):

1. Prefer a **private** report to the maintainer (GitHub Security Advisory when the repo is public, or a direct message).
2. Please do not open a public issue with live secrets attached.
3. Include steps to reproduce and affected versions when possible.

We will aim to acknowledge reports and ship fixes promptly for confirmed issues.
