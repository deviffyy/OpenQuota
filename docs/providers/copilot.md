# GitHub Copilot

OpenQuota tracks the usage and quota information available for your GitHub Copilot account.

## What it tracks

| Metric      | Meaning                                                             |
| ----------- | ------------------------------------------------------------------- |
| Credits     | Included premium-request or AI-credit usage                         |
| Extra Usage | Additional usage reported after the included allowance              |
| Org Credits | Organization-wide credit usage when billing access is available     |
| Org Spend   | Organization-wide additional spend when billing access is available |
| Chat        | Chat quota for accounts where GitHub reports one                    |
| Completions | Completion quota for accounts where GitHub reports one              |

The available metrics depend on the Copilot plan and whether the account is managed by an
organization.

## Sign-in and local data

OpenQuota first looks for credentials left by Copilot integrations, then checks GitHub CLI
authentication. Signing in to Copilot in a supported editor is usually enough. You can also run
`gh auth login`.

Organization billing metrics require a GitHub token with access to the relevant organization's
billing information. These values describe the organization, not an individual seat.

## Troubleshooting

- **Sign in to GitHub Copilot** — sign in through your editor or run `gh auth login`.
- **Token invalid or expired** — authenticate again with GitHub.
- **Some metrics show No data** — the account or plan may not expose those quota fields.
- **Organization metrics are missing** — confirm that the signed-in account can view organization
  billing.
