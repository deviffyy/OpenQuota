# Codex

OpenQuota tracks Codex subscription limits and usage recorded by the Codex CLI.

## What it tracks

| Metric                           | Meaning                                                      |
| -------------------------------- | ------------------------------------------------------------ |
| Session                          | Usage remaining in the current session window                |
| Weekly                           | Usage remaining in the weekly window                         |
| Spark / Spark Weekly             | Model-specific limits when they are reported for the account |
| Extra Usage                      | Additional usage credits reported by Codex                   |
| Rate Limit Resets                | Available reset credits                                      |
| Today / Yesterday / Last 30 Days | Tokens, model usage, and estimated spend from local logs     |
| Usage Trend                      | Recent local usage over time                                 |

## Sign-in and local data

Sign in with the Codex CLI by running `codex` and choosing your ChatGPT account. OpenQuota reads the
same authentication data and respects `CODEX_HOME` when it is set. API-key-only sessions can produce
local usage history, but they cannot provide ChatGPT subscription limits.

Spend history is calculated locally from the Codex `sessions` and `archived_sessions` logs. Compatible
Codex usage recorded by pi can also be included. OpenQuota does not upload these local records.

## Troubleshooting

- **Not logged in** — run `codex`, sign in with ChatGPT, then refresh OpenQuota.
- **Subscription usage unavailable** — replace an API-key-only login with a ChatGPT login.
- **Session expired or revoked** — sign in again with `codex`.
- **No local history** — check the active Codex data directory and the value of `CODEX_HOME`.

## Multi-Account Support

OpenQuota supports multiple Codex accounts through the Hermes credentials pool (`~/.hermes/auth.json`). When using the Hermes file format, OpenQuota detects and monitors all available accounts. 

By default, the `Weekly` metric is pinned to the system tray for all active Codex accounts. The tray icon automatically calculates and displays the average health (remaining limit) across all pinned metrics across all your configured accounts.
