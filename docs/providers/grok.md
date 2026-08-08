# Grok

OpenQuota tracks Grok allowance information and usage recorded by the Grok CLI.

## What it tracks

| Metric                           | Meaning                                               |
| -------------------------------- | ----------------------------------------------------- |
| Weekly                           | Weekly allowance remaining                            |
| Extra Usage                      | Pay-as-you-go availability reported by Grok           |
| Today / Yesterday / Last 30 Days | Tokens and estimated spend calculated from local logs |
| Usage Trend                      | Recent local usage over time                          |

Accounts that still use Grok's older billing model may not report a weekly pool, in which case the
Weekly row shows **No data**.

## Sign-in and local data

Sign in by running `grok login`. OpenQuota reads the authentication data and usage history stored by
the Grok CLI. `GROK_HOME` is respected when it is set.

Spend history is estimated locally from Grok's usage log and is not uploaded by OpenQuota.

## Troubleshooting

- **Not logged in** — run `grok login`, then refresh OpenQuota.
- **Login invalid or expired** — sign in again with the Grok CLI.
- **No local history** — use Grok normally and check the active `GROK_HOME` directory.
- **Billing request failed** — check the connection and try another refresh.
