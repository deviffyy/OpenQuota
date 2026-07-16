<p align="center">
  <img src="assets/openquota-icon.png" alt="OpenQuota logo" width="88">
</p>

<h1 align="center">OpenQuota</h1>

<p align="center">
  Track your AI coding subscriptions from the system tray.
</p>

<p align="center">
  <a href="https://github.com/deviffyy/OpenQuota/actions/workflows/ci.yml"><img src="https://github.com/deviffyy/OpenQuota/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/deviffyy/OpenQuota/releases/latest"><img src="https://img.shields.io/github/v/release/deviffyy/OpenQuota" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license"></a>
</p>

OpenQuota keeps your session limits, weekly quotas, reset times, token usage, and estimated spend
in one compact panel. Pin the metrics you care about to the tray and see what remains without
interrupting your work.

<p align="center">
  <img src="assets/screenshot.png" alt="OpenQuota dashboard showing AI provider quotas, usage, and estimated spend" width="720">
</p>

## Download

| Platform | Available builds                           | Download                                                                      |
| -------- | ------------------------------------------ | ----------------------------------------------------------------------------- |
| Windows  | x64 and ARM64 installers                   | [Download for Windows](https://github.com/deviffyy/OpenQuota/releases/latest) |
| macOS    | Universal DMG for Apple Silicon and Intel  | [Download for macOS](https://github.com/deviffyy/OpenQuota/releases/latest)   |
| Linux    | x64 and ARM64 AppImage and Debian packages | [Download for Linux](https://github.com/deviffyy/OpenQuota/releases/latest)   |

Open the latest release and choose the file for your platform:

- **Windows:** `_x64-setup.exe` or `_arm64-setup.exe`
- **macOS:** `_universal.dmg` — requires macOS 11 or later
- **Linux:** `.AppImage` or `.deb`

OpenQuota checks for updates automatically. Installable updates are cryptographically signed.

## Supported providers

- **Claude Code** — session and weekly limits, model-specific usage, token history, and estimated
  spend
- **Codex** — session and weekly limits, credits, token history, model breakdown, and estimated
  spend
- **Cursor** — total, Auto and API usage, credits, token history, and estimated spend
- **Antigravity** — shared Gemini and Claude quota pools

OpenQuota uses the provider sign-ins already available on your computer. Sign in through the
provider's app or CLI first, then open OpenQuota.

Codex subscription limits require a ChatGPT login. API-key-only sessions do not expose subscription
quota information.

## Features

- **Tray dashboard.** View all provider quotas and reset times in a compact popup.
- **Pinned metrics.** Keep important values visible in the tray or macOS menu bar.
- **Used or left.** Display how much quota you have consumed or how much remains.
- **Usage history.** Review today, yesterday, and the last 30 days of token usage and estimated
  spend.
- **Pacing alerts.** See whether your current usage is likely to last until the next reset.
- **Custom layouts.** Reorder providers and metrics, hide rows, and choose what stays visible.
- **Desktop integration.** Launch at login, use a global shortcut, and follow the system theme.
- **Fast refresh.** Cached values appear immediately and providers refresh automatically in the
  background.

OpenQuota runs locally and has no account, cloud backend, analytics, or usage telemetry of its own.

## Development

Requirements:

- Node.js 22 or later
- pnpm 11.11.0
- Stable Rust toolchain
- [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/)

Install dependencies and start the development app:

```sh
corepack pnpm install --frozen-lockfile
corepack pnpm tauri dev
```

Run the complete quality checks:

```sh
corepack pnpm verify
```

Build an installer for the current platform:

```sh
corepack pnpm build:installer             # Windows
corepack pnpm build:linux                 # Linux
corepack pnpm tauri build --bundles dmg   # macOS
```

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before contributing,
and report security problems privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
