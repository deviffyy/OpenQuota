# Z.ai

OpenQuota tracks quota information for the Z.ai GLM Coding Plan, including plans purchased through
the global Z.ai service and Zhipu AI's BigModel service in China.

## What it tracks

| Metric       | Meaning                                      |
| ------------ | -------------------------------------------- |
| Session      | Usage remaining in the rolling 5-hour window |
| Weekly       | Usage remaining in the rolling 7-day window  |
| Web Searches | Monthly web-search allowance remaining       |

## Setup

Add a Z.ai API key from **Customize** in OpenQuota. Saved keys are kept in the operating system's
credential store. OpenQuota also checks `ZAI_API_KEY`, `GLM_API_KEY`,
`~/.config/openquota/zai.json`, and `~/.config/zai/key.json`; a key saved in the app takes priority.

The key must belong to an account with an active GLM Coding Plan.

OpenQuota first checks the global `api.z.ai` service. If that service rejects the key, it
automatically retries the `open.bigmodel.cn` China service. No region setting is required.

## Troubleshooting

- **Add an API key** — add a key in Customize or provide one through a supported external source.
- **API key invalid** — verify the key at [Z.ai API Keys](https://z.ai/manage-apikey/apikey-list).
- **China API key invalid** — verify the key in the
  [BigModel API key console](https://open.bigmodel.cn/usercenter/apikeys).
- **No active coding plan** — confirm that the account has an active GLM Coding Plan.
- **Usage unavailable** — check the connection and refresh again.
