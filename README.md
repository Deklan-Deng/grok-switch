# Grok Switch

Desktop app for switching **Grok Build CLI** model providers (API base URL, model id, key, `api_backend`) without hand-editing config every time.

Inspired by [CC Switch](https://github.com/farion1231/cc-switch). Built with **Tauri 2 + React + Tailwind**.

> **Unofficial community tool.** Not affiliated with, endorsed by, or sponsored by xAI.  
> “Grok” and related marks belong to their respective owners.

## Features

- Provider cards: add / edit / delete, one-click **Enable** → writes Grok `config.toml`
- Import existing `[model.*]` sections from config
- Tokens stay in a **local vault** until you enable a provider
- One-click **speed test** (`/models` + TTFT + total latency + 403/CF hints)
- Tray support and tools (sessions, doctor, open config)
- **Auto-update** on macOS & Windows (GitHub Releases + signed installers)

## Platform

| Platform | Status |
|----------|--------|
| **macOS** 11+ | Supported |
| **Windows** 10/11 | Supported (WebView2) |
| Linux | Not targeted yet |

## Security (short)

- Tokens (local vault):
  - macOS: `~/Library/Application Support/GrokTokenSwitcher/tokens.json`
  - Windows: `%APPDATA%\GrokTokenSwitcher\tokens.json`
- Grok CLI config: `~/.grok/config.toml` (Windows: `%USERPROFILE%\.grok\config.toml`)
- Live key in Grok config only after **Enable**
- Speed/health probes call **only** the `base_url` you set
- No third-party relay is hard-coded; form examples are empty + xAI **placeholders** only

See [SECURITY.md](./SECURITY.md) for details.

## Requirements (from source)

- Node.js 18+
- Rust stable
- **macOS** 11+ or **Windows** 10/11 (WebView2 runtime; bootstrapper can install it)
- Optional: [Grok CLI](https://docs.x.ai/) for toolbox launch / doctor checks

## Develop

```bash
cd app
npm install
npm run tauri dev
```

## Build

```bash
cd app
npm install
npm run tauri build
```

Artifacts under `app/src-tauri/target/release/bundle/`:

| Platform | Outputs |
|----------|---------|
| macOS | `.app` / `.dmg` |
| Windows | `.msi` / NSIS `.exe` installer |

### macOS notes

Unsigned builds may need System Settings → Privacy & Security, or:

```bash
xattr -cr "/path/to/Grok Switch.app"
```

### Windows notes

- Build on a Windows machine (or a Windows CI runner) with the [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) prerequisites Tauri documents.
- Grok config defaults to `%USERPROFILE%\.grok\config.toml`.
- Toolbox “打开 Grok” prefers **Windows Terminal** (`wt.exe`), otherwise a new `cmd.exe` window.

## Auto-update (macOS + Windows)

In-app updater uses [Tauri Updater](https://v2.tauri.app/plugin/updater/):

1. App checks  
   `https://github.com/Deklan-Deng/grok-switch/releases/latest/download/latest.json`
2. Footer shows version + **检查更新**; startup also checks silently after a few seconds
3. Artifacts must be **signed** with the minisign private key; public key is embedded in `app/src-tauri/tauri.conf.json`

### Signing keys (maintainers)

Private key was generated once on a maintainer machine (example path `~/.tauri/grok-switch.key`). **Never commit the private key.**

```bash
# Generate a new pair only if you intentionally rotate keys
# (rotating invalidates updates for already-installed builds that embed the old pubkey)
cd app
npx tauri signer generate -w ~/.tauri/grok-switch.key --ci -f

# Build release with signatures
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/grok-switch.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""   # if empty password
npm run tauri build
```

GitHub Actions (`.github/workflows/release.yml`) expects secrets:

| Secret | Value |
|--------|--------|
| `TAURI_SIGNING_PRIVATE_KEY` | Full private key file contents |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password (use empty secret if none) |

Publish flow:

```bash
# bump version in app/package.json + app/src-tauri/tauri.conf.json + Cargo.toml
git tag v0.1.1
git push origin v0.1.1
```

CI builds macOS (arm64 + x64) and Windows, drafts a GitHub Release, and uploads installers + `latest.json` for the updater.

> Until the first signed release exists, **检查更新** may report that update metadata is unavailable — that is expected.

## Usage

1. Launch Grok Switch.
2. **Import** from `~/.grok/config.toml`, or **Add** a provider.
3. Fill fields (placeholders show official xAI-style examples; they are not pre-filled values).
4. Save the token locally, then click **Enable** to write config.
5. Use the gauge icon on a card to **speed-test** that provider.

Default `api_backend` for new providers: `responses` (xAI-oriented). You can switch to `chat_completions` or `messages`.

## Project layout

```text
.
├── LICENSE
├── README.md
├── SECURITY.md
└── app/                 # Tauri + React application
    ├── src/             # Frontend
    └── src-tauri/       # Rust backend
```

## License

[MIT](./LICENSE)

## Disclaimer

This project is independent software for managing local Grok Build CLI configuration.  
It is not an official xAI product. Use third-party API endpoints at your own risk.
