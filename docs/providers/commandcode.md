# Command Code

OpenQuota reads the local session created by the Command Code CLI and tracks the subscription
windows reported by Command Code.

## What it tracks

| Metric          | Meaning                                                    |
| --------------- | ---------------------------------------------------------- |
| Session         | Credit usage in the rolling 5-hour subscription window    |
| Weekly          | Credit usage in the rolling 7-day subscription window     |
| Monthly Credits | Remaining credits from the current subscription allocation |
| Extra Credits   | Remaining purchased or top-up credits, when available      |

## Setup

Install the Command Code CLI and sign in:

```sh
command-code login
```

OpenQuota reads `~/.commandcode/auth.json` locally. The session key remains on your device and is
used only to request Command Code usage data.

## Troubleshooting

- **Not logged in** — run `command-code login`, then refresh OpenQuota.
- **Login expired** — sign in again with `command-code login`.
- **Usage unavailable** — check the connection and refresh again.
