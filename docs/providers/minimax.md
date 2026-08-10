# MiniMax

OpenQuota tracks the Session and Weekly quotas of a MiniMax Token Plan.

## What it tracks

| Metric  | Meaning                                      |
| ------- | -------------------------------------------- |
| Session | Usage remaining in the rolling 5-hour window |
| Weekly  | Usage remaining in the rolling 7-day window  |

## Setup

Create or view the Token Plan subscription key in the [MiniMax global console](https://platform.minimax.io/console/plan),
then add it in **Customize** in OpenQuota. Saved keys are stored in the operating system's credential
store. OpenQuota also checks `MINIMAX_API_KEY` and `~/.config/openquota/minimax.json`; a key saved
in the app takes priority.

This provider uses MiniMax's global endpoint, `https://www.minimax.io/v1/token_plan/remains`. Use a
key from the global console; keys from the mainland China platform are a separate account system.

## Troubleshooting

- **Add an API key** — add a MiniMax Token Plan key in Customize or provide it through a supported
  external source.
- **No active token plan** — subscribe to a Token Plan in the
  [MiniMax global console](https://platform.minimax.io/console/plan).
- **Usage unavailable** — check the connection and refresh again.
