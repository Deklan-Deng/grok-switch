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
- Config target: `~/.grok/config.toml` for Grok Build CLI

## Notes

- New provider form fields start **empty**; xAI strings are **placeholders** only.
- Default `api_backend` selection: `responses`.
- Tokens are not written to `config.toml` until you **Enable** a provider.
