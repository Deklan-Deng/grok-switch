# Grok Switch (app)

Application sources for **Grok Switch**. Project overview, license, and security notes live in the [repository root](../README.md).

## Quick start

```bash
npm install
npm run tauri dev
```

```bash
npm run tauri build
```

## Stack

- Frontend: React + TypeScript + Tailwind + shadcn-style UI
- Backend: Tauri 2 (Rust)
- Config target: `~/.grok/config.toml` (Windows: `%USERPROFILE%\.grok\config.toml`)
- Platforms: macOS 11+ and Windows 10/11 (WebView2)

## Notes

- New provider form fields start **empty**; xAI strings are **placeholders** only.
- Default `api_backend` selection: `responses`.
- Tokens are not written to `config.toml` until you **Enable** a provider.
- Windows toolbox opens Grok in Windows Terminal when available, else `cmd.exe`.
- Auto-update via Tauri updater + GitHub Releases `latest.json` (root README).
