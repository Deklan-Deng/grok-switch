# Security

Grok Switch is a **local desktop utility**. It does not run a cloud backend and does not phone home.

## What is stored where

| Data | Location | Notes |
|------|----------|--------|
| Provider list / settings | macOS: `~/Library/Application Support/GrokTokenSwitcher/profiles.json`<br>Windows: `%APPDATA%\GrokTokenSwitcher\profiles.json` | No API keys in normal flow |
| API tokens | Same directory as `tokens.json` | Local file; mode `0600` on Unix when written |
| Active Grok CLI config | macOS/Linux: `~/.grok/config.toml`<br>Windows: `%USERPROFILE%\.grok\config.toml` | Written **only when you enable** a provider |

On macOS, legacy Keychain entries (if any from older builds) may be migrated once into the local token vault.

## Network behavior

- **Enable / import / edit**: local filesystem only (plus optional `grok` CLI on your machine).
- **Health check / speed test**: HTTPS requests to the `base_url` you configured (e.g. `https://api.x.ai/v1` or a third-party compatible endpoint). Your API key is sent as `Authorization: Bearer …` to that host only.
- **Auto-update** (optional, user-visible): on startup and via footer **检查更新**, the app may request  
  `https://github.com/Deklan-Deng/grok-switch/releases/latest/download/latest.json`  
  and, if the user accepts an update, download signed installer artifacts from GitHub Releases.  
  Update packages are verified with the embedded minisign **public** key before install.  
  No analytics or crash telemetry is sent.
- The updater **private** signing key must never be committed or shared; only maintainers who publish releases need it.

## What you should never commit

## What you should never commit

Do not put any of the following in git or screenshots of public issues:

- Real API keys / tokens
- Your personal `~/.grok/config.toml`
- `tokens.json` / `profiles.json` from Application Support
- Private relay hostnames you do not want public (optional; keys matter more)
- Updater **private** signing key (`TAURI_SIGNING_PRIVATE_KEY` / `*.key`)

## Reporting a vulnerability

If you find a security issue (for example token leakage, path traversal, or unsafe shell use):

1. Prefer a **private** report to the maintainer (GitHub Security Advisory when the repo is public, or a direct message).
2. Please do not open a public issue with live secrets attached.
3. Include steps to reproduce and affected versions when possible.

We will aim to acknowledge reports and ship fixes promptly for confirmed issues.
