# Railway service definitions

The explorer runs on Railway as four services in one project: a Postgres
database, the Blockscout backend, the Next.js frontend, and (optionally) the
two Rust microservices under `explorer/services`. The two JSON files beside
this one hold the build and deploy configuration for the backend and the
frontend; everything else is a per-service setting or a variable and is listed
below.

Railway's config-as-code format (`railway.json` / `railway.toml`) is on its way
out in favour of Infrastructure as Code (`.railway/railway.ts`). Existing
services keep reading these files until the cutoff Railway announces in its own
documentation; the fields used here are the ones both formats express, so the
move is a mechanical translation.

## Per-service settings

These three cannot be expressed in `railway.json` and have to be set once in
the service settings.

| Service | Root directory | Config file path | Public port |
| --- | --- | --- | --- |
| backend | `explorer/backend` | `/explorer/deploy/railway/backend.railway.json` | 4000 |
| frontend | `explorer/frontend` | `/explorer/deploy/railway/frontend.railway.json` | 3000 |

The config file path is resolved from the repository root and deliberately does
not follow the root directory; `dockerfilePath` inside each file *is* resolved
from the root directory, and `watchPatterns` are gitignore-style patterns
resolved from the repository root.

Railway injects `PORT` into every container. Both images honour it: the
Blockscout release reads `PORT` (4000 in these definitions) and the frontend
image reads `PORT` (3000). Set the value explicitly as a service variable so
the container port and the service's target port agree.

## What each file says

`backend.railway.json` builds `docker/Dockerfile` from the backend tree and
starts with

```
/bin/sh -c "bin/blockscout eval 'Elixir.Explorer.ReleaseTasks.create_and_migrate()' && bin/blockscout start"
```

so the database is created and migrated in the same step that boots the
release. The health check is `GET /api/health/liveness`, which the API router
serves from `BlockScoutWeb.API.HealthController`. The timeout is generous
because the first deploy of a fresh database runs every migration before the
endpoint answers.

`frontend.railway.json` builds `Dockerfile` from the frontend tree and sets no
start command on purpose: the image has an `ENTRYPOINT` (`entrypoint.sh`) that
validates the `NEXT_PUBLIC_*` variables, generates the client-side env script,
the favicon and the sitemap, and then runs `node server.js`. Overriding the
start command would skip that. The health check is `GET /api/healthz`, the
Next.js route in `explorer/frontend/pages/api/healthz.tsx`.

## Database

Use a Railway Postgres service and give the backend its connection string in
`DATABASE_URL`. The backend release has no `sslmode` default of its own, so set
`ECTO_USE_SSL` to match how the database service is reached.

## Microservices

`smart-contract-verifier` and `sig-provider` build from
`explorer/services/<name>/Dockerfile` with the service's root directory set to
that same path; neither has a path dependency outside its own directory. They
listen on 8050 and take no config file, so they need no `railway.json` — set
`SMART_CONTRACT_VERIFIER__SERVER__HTTP__ADDR` / `SIG_PROVIDER__SERVER__HTTP__ADDR`
and point the backend's `MICROSERVICE_SC_VERIFIER_URL` and
`MICROSERVICE_SIG_PROVIDER_URL` at their private network addresses.

## Variable names

Names only. Every value is deployment-specific and belongs in Railway's
variable store, never in this repository. Where an example is unavoidable the
placeholder form is `<...>`.

### Backend

All of these names appear in
`explorer/backend/docker-compose/envs/common-blockscout.env`, which is the
complete upstream catalogue of backend variables.

Connection and identity — no default, the deployment must supply each one:

| Name | What it selects |
| --- | --- |
| `DATABASE_URL` | Postgres connection string for the indexer and the API |
| `SECRET_KEY_BASE` | Phoenix signing secret, at least 64 bytes |
| `PORT` | port the release listens on |
| `CHAIN_ID` | EIP-155 chain id of the indexed network |
| `ETHEREUM_JSONRPC_HTTP_URL` | JSON-RPC HTTP endpoint of the node |
| `ETHEREUM_JSONRPC_TRACE_URL` | endpoint used for tracing calls |
| `ETHEREUM_JSONRPC_ETH_CALL_URL` | endpoint used for `eth_call` |
| `ETHEREUM_JSONRPC_WS_URL` | websocket endpoint used for `newHeads` |
| `BLOCKSCOUT_HOST` | public host the API is served from |
| `BLOCKSCOUT_PROTOCOL` | `http` or `https` for that host |
| `API_URL` | public base URL of the API |
| `WEBAPP_URL` | public base URL of the frontend |
| `CHECK_ORIGIN` | origins accepted on the websocket |

Behaviour — these have upstream defaults, and the deployment sets them to pin
the shape of the explorer:

| Group | Names |
| --- | --- |
| transport | `ETHEREUM_JSONRPC_VARIANT`, `ETHEREUM_JSONRPC_TRANSPORT`, `ETHEREUM_JSONRPC_DISABLE_ARCHIVE_BALANCES`, `ETHEREUM_JSONRPC_HTTP_TIMEOUT`, `ETHEREUM_JSONRPC_FALLBACK_HTTP_URL` |
| database | `POOL_SIZE`, `POOL_SIZE_API`, `ECTO_USE_SSL`, `DATABASE_QUEUE_TARGET`, `DATABASE_READ_ONLY_API_URL` |
| chain labels | `CHAIN_TYPE`, `NETWORK`, `SUBNETWORK`, `COIN`, `COIN_NAME`, `DISABLE_MARKET` |
| API surface | `API_V2_ENABLED`, `API_V1_READ_METHODS_DISABLED`, `API_V1_WRITE_METHODS_DISABLED`, `DISABLE_WEBAPP`, `ADMIN_PANEL_ENABLED`, `API_RATE_LIMIT_DISABLED`, `RE_CAPTCHA_DISABLED`, `CHECKSUM_ADDRESS_HASHES` |
| indexer | `DISABLE_INDEXER`, `DISABLE_REALTIME_INDEXER`, `DISABLE_CATCHUP_INDEXER`, `INDEXER_CATCHUP_BLOCKS_BATCH_SIZE`, `INDEXER_CATCHUP_BLOCKS_CONCURRENCY`, `INDEXER_RECEIPTS_BATCH_SIZE`, `INDEXER_RECEIPTS_CONCURRENCY`, `INDEXER_COIN_BALANCES_BATCH_SIZE`, `INDEXER_COIN_BALANCES_CONCURRENCY`, `INDEXER_REALTIME_FETCHER_MAX_GAP`, `INDEXER_REALTIME_FETCHER_POLLING_PERIOD` |
| fetchers off | `INDEXER_DISABLE_INTERNAL_TRANSACTIONS_FETCHER`, `INDEXER_DISABLE_BLOCK_REWARD_FETCHER`, `INDEXER_DISABLE_ARCHIVAL_TOKEN_BALANCES_FETCHER`, `INDEXER_DISABLE_WITHDRAWALS_FETCHER` |
| microservices | `MICROSERVICE_SC_VERIFIER_ENABLED`, `MICROSERVICE_SC_VERIFIER_URL`, `MICROSERVICE_SC_VERIFIER_TYPE`, `MICROSERVICE_SIG_PROVIDER_ENABLED`, `MICROSERVICE_SIG_PROVIDER_URL`, `MICROSERVICE_VISUALIZE_SOL2UML_ENABLED`, `MICROSERVICE_VISUALIZE_SOL2UML_URL` |
| optional Redis | `API_RATE_LIMIT_HAMMER_REDIS_URL`, `ACCOUNT_REDIS_URL` |
| accounts and media | `ACCOUNT_ENABLED`, `NFT_MEDIA_HANDLER_ENABLED` |
| runtime | `DISABLE_FILE_LOGGING`, `HEART_BEAT_TIMEOUT`, `TXS_STATS_DAYS_TO_COMPILE_AT_INIT`, `COIN_BALANCE_HISTORY_DAYS` |

Without `API_RATE_LIMIT_HAMMER_REDIS_URL` the rate limiter falls back to an
in-process ETS table, which is correct for a single replica and wrong for
several; add Redis before scaling the backend out.

### Frontend

These names appear in
`explorer/backend/docker-compose/envs/common-frontend.env`:

| Name | What it selects |
| --- | --- |
| `NEXT_PUBLIC_API_HOST` | host the browser calls for the API |
| `NEXT_PUBLIC_API_PROTOCOL` | `http` or `https` for that host |
| `NEXT_PUBLIC_API_BASE_PATH` | path prefix the API is mounted under |
| `NEXT_PUBLIC_API_WEBSOCKET_PROTOCOL` | `ws` or `wss` |
| `NEXT_PUBLIC_APP_HOST` | host the frontend itself is served from |
| `NEXT_PUBLIC_APP_PROTOCOL` | `http` or `https` for that host |
| `NEXT_PUBLIC_NETWORK_NAME` | full network name in the interface |
| `NEXT_PUBLIC_NETWORK_SHORT_NAME` | short name used where space is tight |
| `NEXT_PUBLIC_NETWORK_ID` | EIP-155 chain id, same value as `CHAIN_ID` |
| `NEXT_PUBLIC_NETWORK_CURRENCY_NAME` | native coin name |
| `NEXT_PUBLIC_NETWORK_CURRENCY_SYMBOL` | native coin ticker |
| `NEXT_PUBLIC_NETWORK_CURRENCY_DECIMALS` | decimals the interface formats with |
| `NEXT_PUBLIC_HOMEPAGE_CHARTS` | charts shown on the home page |
| `NEXT_PUBLIC_IS_TESTNET` | marks the interface as a test network |
| `NEXT_PUBLIC_API_SPEC_URL` | OpenAPI document linked from the API page |
| `NEXT_PUBLIC_STATS_API_HOST` | stats microservice, only if one is deployed |
| `NEXT_PUBLIC_VISUALIZE_API_HOST` | visualizer microservice, only if deployed |
| `NEXT_PUBLIC_WALLET_CONNECT_PROJECT_ID` | required for wallet interaction |

Four more names are not in that file but are needed whenever the API and the
frontend are not behind one proxy on the default port. They are part of the
frontend's own catalogue, `explorer/frontend/docs/ENVS.md`:
`NEXT_PUBLIC_API_PORT`, `NEXT_PUBLIC_APP_PORT`, `NEXT_PUBLIC_AD_BANNER_PROVIDER`,
`NEXT_PUBLIC_AD_TEXT_PROVIDER`.

The frontend image validates this set on start against
`deploy/tools/envs-validator`. A name the schema does not know, or a required
name left empty, stops the container rather than being ignored, so a typo in
the Railway variable store surfaces as a failed deploy and not as a subtly
wrong page.
