# Kimi

OpenQuota tracks the Session (rolling five-hour) and Weekly quotas of a Kimi Code membership.

## What it tracks

| Metric  | Meaning                                      |
| ------- | -------------------------------------------- |
| Session | Usage remaining in the rolling 5-hour window |
| Weekly  | Usage remaining in the rolling 7-day window  |

## Setup

Create a Kimi Code API key in the [Kimi Code Console](https://www.kimi.com/code/console), then add
it in **Customize** in OpenQuota. Saved keys are stored in the operating system's credential store.
OpenQuota also checks `KIMI_API_KEY` and `~/.config/openquota/kimi.json`; a key saved in the app
takes priority.

This provider uses the Kimi Code endpoint, `https://api.kimi.com/coding/v1`. Kimi Code keys are not
interchangeable with Kimi Open Platform keys.

## Troubleshooting

- **Add an API key** — add a Kimi Code key in Customize or provide it through a supported external source.
- **API key invalid** — create or verify the key in the [Kimi Code Console](https://www.kimi.com/code/console).
- **Usage unavailable** — check the connection and refresh again.
