# Devin

OpenQuota tracks quota and balance information for the Devin account signed in on your computer.

## What it tracks

| Metric        | Meaning                                    |
| ------------- | ------------------------------------------ |
| Daily         | Usage remaining in the daily window        |
| Weekly        | Usage remaining in the weekly window       |
| Extra Balance | Additional usage balance reported by Devin |

Reset times are shown when Devin includes them in the account response.
If Devin hides the daily quota and does not report a separate weekly value, OpenQuota uses the
remaining daily value for the Weekly row.

## Sign-in and local data

Sign in through the Devin app or run `devin auth login`. OpenQuota checks the local credentials
created by the Devin CLI and the signed-in state maintained by the desktop app. The correct
platform-specific locations are selected automatically.

## Troubleshooting

- **Not logged in** — run `devin auth login` or sign in to the Devin app.
- **Login expired** — authenticate again, then refresh OpenQuota.
- **Quota unavailable** — the signed-in account may not expose quota data.
- **Could not reach Devin** — check the connection and try another refresh.
