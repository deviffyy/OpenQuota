# OpenRouter

OpenQuota tracks account credits, balance, and spending through an OpenRouter API key.

## What it tracks

| Metric     | Meaning                                            |
| ---------- | -------------------------------------------------- |
| Credits    | Lifetime spend against the total credits purchased |
| Balance    | Remaining OpenRouter balance                       |
| Today      | Spend reported for the current day                 |
| This Week  | Spend reported for the current week                |
| This Month | Spend reported for the current month               |
| Key Limit  | Limit assigned to the active API key, when present |

## Setup

Add an OpenRouter API key from **Customize** in OpenQuota. Saved keys are kept in the operating
system's credential store. OpenQuota can also use `OPENROUTER_API_KEY` or
`OPENROUTER_KEY`, or read `~/.config/openrouter/key.json`; a key saved in the app takes priority.

## Troubleshooting

- **Add an API key** — add a key in Customize or provide one through a supported external source.
- **API key invalid** — create or verify the key at [OpenRouter Keys](https://openrouter.ai/keys).
- **Key limit missing** — OpenRouter does not report a limit for every key.
- **Usage unavailable** — check the connection and refresh again.
