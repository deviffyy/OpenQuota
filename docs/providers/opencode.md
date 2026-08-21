# OpenCode

OpenQuota combines OpenCode Go quota information with usage recorded by local OpenCode sessions.

## What it tracks

| Metric                           | Meaning                                           |
| -------------------------------- | ------------------------------------------------- |
| Session                          | OpenCode Go rolling-window usage                  |
| Weekly                           | OpenCode Go weekly usage                           |
| Monthly                          | OpenCode Go monthly usage                          |
| Today / Yesterday / Last 30 Days | Local hosted usage and spend recorded by OpenCode |
| Usage Trend                      | Recent local usage over time                      |

Go quota rows appear when a compatible OpenCode Go login is available. Local history can still be
shown when OpenCode has been used without that plan.

The Go meters come from OpenCode's account usage endpoint, so they include usage from all devices
and reflect the limits enforced by OpenCode. Local usage history remains separate and is read from
the OpenCode data directory.

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
