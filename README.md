# Grok Switch

Desktop app for switching **Grok Build CLI** model providers (API base URL, model id, key, `api_backend`) without hand-editing config every time.

Inspired by [CC Switch](https://github.com/farion1231/cc-switch). Built with **Tauri 2 + React + Tailwind**.

> **Unofficial community tool.** Not affiliated with, endorsed by, or sponsored by xAI.  
> “Grok” and related marks belong to their respective owners.

## Features

- Provider cards: add / edit / delete, one-click **Enable** → writes `~/.grok/config.toml`
- Import existing `[model.*]` sections from config
- Tokens stay in a **local vault** until you enable a provider
- Backup before overwrite; restore-friendly paths
- One-click **speed test** (`/models` + TTFT + total latency + 403/CF hints)
- Tray support and environment toolbox (local checks)

## Platform

| Platform | Status |
|----------|--------|
| **macOS** 11+ | Primary target (build & use) |
| Windows | Scaffolded in deps; not the current ship target |
| Linux | Not targeted yet |

## Security (short)

- Tokens: `~/Library/Application Support/GrokTokenSwitcher/tokens.json`
- Live key in Grok config only after **Enable**
- Speed/health probes call **only** the `base_url` you set
- No third-party relay is hard-coded; form examples are empty + xAI **placeholders** only

See [SECURITY.md](./SECURITY.md) for details.

## Requirements (from source)

- Node.js 18+
- Rust stable
- macOS 11+ (for the current app target)
- Optional: [Grok CLI](https://docs.x.ai/) installed if you use CLI verification tools

## Develop

```bash
cd app
npm install
npm run tauri dev
```

## Build (macOS)

```bash
cd app
npm install
npm run tauri build
```

Artifacts under `app/src-tauri/target/release/bundle/`.

Unsigned builds may need System Settings → Privacy & Security, or:

```bash
xattr -cr "/path/to/Grok Switch.app"
```

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
