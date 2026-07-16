# Contributing to OpenQuota

Thanks for helping improve OpenQuota. Bug reports, feature ideas, documentation fixes, and focused
pull requests are welcome.

## Project philosophy

OpenQuota aims for clean design, fast performance, and a focused user experience. Its purpose is
simple: make AI coding subscription usage and quota limits easy to understand without interrupting
the user's work.

New features should fit that purpose, follow the existing architecture, and remain useful across the
supported desktop platforms where applicable. Prefer clear, maintainable solutions over unnecessary
abstractions, dependencies, or complexity. Keep changes small and focused; do not over-engineer.

## Before opening an issue

- Search existing issues and pull requests first.
- Use the latest OpenQuota release when reproducing a bug.
- Include your operating system, architecture, OpenQuota version, and affected provider.
- Never post credentials, access tokens, account identifiers, or unreviewed diagnostic logs.

Security vulnerabilities should not be reported in a public issue. Follow
[SECURITY.md](SECURITY.md) instead.

## Development setup

You need Node.js 22 or later, pnpm 11.11.0, stable Rust, and the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

```sh
corepack pnpm install --frozen-lockfile
corepack pnpm tauri dev
```

Run the complete quality checks before submitting a pull request:

```sh
corepack pnpm verify
```

## Pull requests

- Keep each pull request focused on one change.
- Explain the problem and the chosen solution.
- Add or update tests when behavior changes.
- Include screenshots for visible interface changes.
- Keep platform-specific behavior working on Windows, macOS, and Linux where applicable.
- Do not include unrelated formatting, generated build output, credentials, or local configuration.

By contributing, you agree that your contribution is licensed under the repository's
[MIT License](LICENSE).
