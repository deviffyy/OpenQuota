# OpenCode

OpenQuota combines OpenCode Go quota information with usage recorded by local OpenCode sessions.

## What it tracks

| Metric                           | Meaning                                           |
| -------------------------------- | ------------------------------------------------- |
| Session                          | OpenCode Go session allowance remaining           |
| Weekly                           | OpenCode Go weekly allowance remaining            |
| Monthly                          | OpenCode Go monthly spend allowance remaining     |
| Today / Yesterday / Last 30 Days | Local hosted usage and spend recorded by OpenCode |
| Usage Trend                      | Recent local usage over time                      |

Go quota rows appear when a compatible OpenCode Go login is available. Local history can still be
shown when OpenCode has been used without that plan.

The Go meters compare usage recorded on this computer with the plan caps. Usage from another device
or a session that has not yet been written locally can make them lower than the account-wide total.
The displayed costs come from the values recorded by OpenCode rather than being estimated by
OpenQuota.

## Sign-in and local data

Sign in to OpenCode Go or use OpenCode locally first. OpenQuota reads OpenCode's local authentication
file and databases from its data directory. `OPENCODE_DATA_DIR` and `XDG_DATA_HOME` are respected
when present.

## Troubleshooting

- **OpenCode was not detected** — sign in to OpenCode Go or complete a local OpenCode session.
- **Login data could not be read** — sign in to OpenCode Go again.
- **Data directory could not be read** — check the configured data directory and its permissions.
- **Local usage unavailable** — check that the OpenCode data directory and database are readable,
  then refresh.
