# Layerx-protocol — Agent Context

<!-- codify-owned: graph-agent-context v1 -->

_Generated graph context owned by `cg agentmd`. Regenerate with `cg agentmd --write` after significant changes. Workflow instructions remain owned by `cg spec render`._

## Languages

| Language | Files | Lines |
|---|---:|---:|
| go | 4168 | 1047081 |
| rust | 901 | 365886 |
| c | 506 | 117986 |
| solidity | 469 | 55873 |
| typescript | 410 | 71492 |
| javascript | 88 | 15191 |
| java | 77 | 15508 |
| python | 59 | 11468 |
| swift | 32 | 6811 |
| csharp | 13 | 4345 |
| kotlin | 5 | 235 |

6728 source files, 1711876 lines total.

## Directory map

- `agent/` — 388 files, 102503 lines (mostly rust)
- `cmd/` — 27 files, 13534 lines (mostly c)
- `contracts/` — 50 files, 4942 lines (mostly solidity)
- `fuzz/` — 11 files, 685 lines (mostly c)
- `human/` — 411 files, 132230 lines (mostly typescript)
- `include/` — 56 files, 7844 lines (mostly c)
- `interop/` — 78 files, 39912 lines (mostly rust)
- `paxeer-network/` — 4838 files, 1143551 lines (mostly go)
- `platform/` — 303 files, 92915 lines (mostly rust)
- `programs/` — 222 files, 93352 lines (mostly rust)
- `scripts/` — 1 files, 847 lines (mostly solidity)
- `src/` — 152 files, 42402 lines (mostly c)
- `spec/` — 4 files, 798 lines (mostly go)
- `test/` — 10 files, 4782 lines (mostly solidity)
- `tests/` — 162 files, 28002 lines (mostly c)
- `tools/` — 15 files, 3577 lines (mostly javascript)

## Build & tooling

- `Makefile` — make
- `package.json` — npm/node
- `foundry.toml` — Foundry (Solidity)

## Entry points

- function `main` — `agent/crates/layerx-agent-api/build.rs:309`
- function `main` — `agent/crates/layerx-agentd/examples/review_audit_export.rs:24`
- function `main` — `agent/crates/layerx-agentd/src/main.rs:538`
- function `main` — `agent/crates/layerx-proof/examples/offline_export.rs:14`
- function `main` — `agent/crates/layerx-proof/examples/offline_verify.rs:68`
- function `main` — `agent/tests/boundary/node/layerxd_lni.c:814`
- function `main` — `agent/tests/boundary/node/preparation_state.c:36`
- function `main` — `agent/tests/boundary/src/main.rs:8`
- function `main` — `agent/tests/isolation/src/main.rs:344`
- function `main` — `agent/tests/parity/python.py:87`
- function `main` — `agent/tests/parity/src/main.rs:957`
- function `main` — `agent/tests/qualify/src/main.rs:19`
- function `main` — `agent/tools/audit-verify/src/main.rs:18`
- function `main` — `agent/tools/boundary-check/src/main.rs:205`
- function `main` — `agent/tools/doc-check/src/main.rs:187`

## HTTP routes

| Method | Pattern | Handler | Where |
|---|---|---|---|
| GET | `` | `version` | `paxeer-network/storage/tools/bench/benchmark.go:155` |
| * | `/api/explorer/verify` | — | `human/apps/web/src/app/api/explorer/verify/route.ts:1` |
| * | `/api/performance/vitals` | — | `human/apps/web/src/app/api/performance/vitals/route.ts:1` |
| * | `/api/support/reports` | — | `human/apps/web/src/app/api/support/reports/route.ts:1` |
| * | `/api/support/reports/[traceId]` | — | `human/apps/web/src/app/api/support/reports/[traceId]/route.ts:1` |
| * | `/app/agents/agt_test_12345678` | — | `human/apps/web/e2e/move.spec.ts:180` |
| * | `/app/withdraw` | — | `human/apps/web/e2e/custody.spec.ts:239` |
| * | `/explorer/lookup` | — | `human/apps/web/src/app/explorer/lookup/route.ts:1` |
| GET | `/integration` | — | `platform/docs/samples/paid-endpoint-spring/src/main/java/com/sidiora/layerx/docs/PaidApiApplication.java:49` |
| * | `/invalid/path` | — | `human/apps/web/e2e/activity.spec.ts:531` |
| GET | `/layerx/integration` | — | `platform/integrations/spring/example/src/main/java/com/sidiora/layerx/example/PaidApiApplication.java:54` |
| GET | `/layerx/mount` | — | `platform/integrations/fastapi/examples/paid_api.py:115` |
| GET | `/layerx/settlements` | — | `platform/integrations/express/example/index.mjs:76` |
| GET | `/layerx/settlements` | — | `platform/integrations/fastapi/examples/paid_api.py:110` |
| GET | `/layerx/settlements` | — | `platform/integrations/spring/example/src/main/java/com/sidiora/layerx/example/PaidApiApplication.java:47` |
| * | `/layerx/webhooks` | — | `platform/docs/samples/paid-route-next/app/layerx/webhooks/route.js:1` |
| * | `/layerx/webhooks` | — | `platform/integrations/next/example/app/layerx/webhooks/route.js:1` |
| GET | `/mount` | — | `platform/docs/samples/paid-endpoint-fastapi/app.py:116` |
| * | `/paid` | — | `platform/docs/samples/paid-route-next/app/paid/route.js:1` |
| * | `/paid` | — | `platform/examples/paid-api/index.mjs:190` |
| * | `/paid` | — | `platform/integrations/next/example/app/paid/route.js:1` |
| GET | `/settlements` | — | `platform/docs/samples/paid-endpoint-express/index.mjs:21` |
| GET | `/settlements` | — | `platform/docs/samples/paid-endpoint-spring/src/main/java/com/sidiora/layerx/docs/PaidApiApplication.java:42` |
| GET | `/settlements` | — | `platform/docs/samples/paid-endpoint-fastapi/app.py:111` |
| * | `/v1/account/balance` | — | `human/apps/web/src/api/generated/index.ts:3823` |
| * | `/v1/accounts` | — | `human/apps/web/src/api/generated/index.ts:3824` |
| * | `/v1/activity/exports/../../../etc/passwd` | — | `human/apps/web/e2e/activity.spec.ts:543` |
| * | `/v1/activity/exports/evidence` | — | `human/apps/web/src/api/generated/index.ts:3826` |
| * | `/v1/activity/exports/exp_abcdefgh/download` | — | `human/apps/web/e2e/activity.spec.ts:521` |
| * | `/v1/activity/exports/statement` | — | `human/apps/web/src/api/generated/index.ts:3827` |
| * | `/v1/activity/query` | — | `human/apps/web/src/api/generated/index.ts:3828` |
| * | `/v1/activity/{entry_id}` | — | `human/apps/web/src/api/generated/index.ts:3825` |
| * | `/v1/agents` | — | `human/apps/web/src/api/generated/index.ts:3830` |
| * | `/v1/agents` | — | `human/apps/web/src/api/generated/index.ts:3833` |
| * | `/v1/agents/{agent_id}` | — | `human/apps/web/src/api/generated/index.ts:3831` |
| * | `/v1/agents/{agent_id}/archive` | — | `human/apps/web/src/api/generated/index.ts:3829` |
| * | `/v1/agents/{agent_id}/limit` | — | `human/apps/web/src/api/generated/index.ts:3832` |
| * | `/v1/agents/{agent_id}/pause` | — | `human/apps/web/src/api/generated/index.ts:3834` |
| * | `/v1/agents/{agent_id}/reclaim` | — | `human/apps/web/src/api/generated/index.ts:3835` |
| * | `/v1/agents/{agent_id}/recover` | — | `human/apps/web/src/api/generated/index.ts:3836` |
| * | `/v1/agents/{agent_id}/resume` | — | `human/apps/web/src/api/generated/index.ts:3837` |
| * | `/v1/agents/{agent_id}/rotate` | — | `human/apps/web/src/api/generated/index.ts:3838` |
| * | `/v1/approvals` | — | `human/apps/web/src/api/generated/index.ts:3841` |
| * | `/v1/approvals/{approval_id}` | — | `human/apps/web/src/api/generated/index.ts:3840` |
| * | `/v1/approvals/{approval_id}/approve` | — | `human/apps/web/src/api/generated/index.ts:3839` |
| * | `/v1/approvals/{approval_id}/reject` | — | `human/apps/web/src/api/generated/index.ts:3842` |
| * | `/v1/deposits` | — | `human/apps/web/src/api/generated/index.ts:3854` |
| * | `/v1/deposits/{journey_id}/confirm` | — | `human/apps/web/src/api/generated/index.ts:3853` |
| * | `/v1/evidence/{evidence_id}` | — | `human/apps/web/src/api/generated/index.ts:3855` |
| * | `/v1/exit` | — | `human/apps/web/src/api/generated/index.ts:3857` |
| * | `/v1/exit/eligibility` | — | `human/apps/web/src/api/generated/index.ts:3856` |
| * | `/v1/home` | — | `human/apps/web/src/api/generated/index.ts:3858` |
| * | `/v1/journeys` | — | `human/apps/web/src/api/generated/index.ts:3860` |
| * | `/v1/journeys/{journey_id}` | — | `human/apps/web/src/api/generated/index.ts:3859` |
| * | `/v1/moves` | — | `human/apps/web/src/api/generated/index.ts:3861` |
| * | `/v1/moves/quote` | — | `human/apps/web/src/api/generated/index.ts:3862` |
| * | `/v1/notifications` | — | `human/apps/web/src/api/generated/index.ts:3863` |
| * | `/v1/notifications/preferences` | — | `human/apps/web/src/api/generated/index.ts:3864` |
| * | `/v1/notifications/preferences` | — | `human/apps/web/src/api/generated/index.ts:3865` |
| * | `/v1/notifications/{notification_id}/read` | — | `human/apps/web/src/api/generated/index.ts:3866` |

## Load-bearing symbols (most referenced)

- `len` (function, 18629 refs) — `agent/crates/layerx-agentd/src/approval/mod.rs:271`
- `Equal` (function, 11049 refs) — `paxeer-network/consensus/internal/protoutils/msg.go:45`
- `uint64` (function, 9375 refs) — `platform/sdk/swift/Sources/LayerXSDK/MirrorSource.swift:395`
- `Err` (method, 9213 refs) — `paxeer-network/consensus/abci/types/types.go:24`
- `new` (function, 8252 refs) — `agent/crates/layerx-agent-api/src/error.rs:20`
- `byte` (function, 7587 refs) — `agent/crates/layerx-agentd/src/audit/record.rs:554`
- `Some` (function, 5273 refs) — `paxeer-network/consensus/libs/utils/option.go:15`
- `String` (method, 5111 refs) — `paxeer-network/consensus/abci/types/types.pb.go:48`
- `and` (method, 5076 refs) — `paxeer-network/consensus/libs/bits/bit_array.go:165`
- `make` (function, 4396 refs) — `platform/integrations/ios/sample-app/Sources/LayerXWalletApp.swift:12`
- `append` (function, 4335 refs) — `human/crates/layerx-human-service/src/activity/mod.rs:1020`
- `Size` (method, 4317 refs) — `paxeer-network/consensus/abci/types/types.pb.go:2309`
- `import` (function, 3951 refs) — `programs/porting/cosmwasm/src/wasm.rs:268`
- `Error` (contract, 3891 refs) — `contracts/libraries/Error.sol:6`
- `int` (function, 3813 refs) — `paxeer-network/sdk/crypto/keys/secp256k1/internal/secp256k1/libsecp256k1/include/secp256k1.h:86`

## Querying this codebase

This project is indexed by Codify (SQLite + FTS5, 100% local). Prefer these over grep/file-walking — one call returns definitions, snippets, and call edges:

```bash
cg context <query>      # symbols + snippets + callers/callees + routes
cg search <text>        # instant name/full-text search
cg symbol <name>        # definition + snippet + reference count
cg impact <name> -d 3   # who breaks if this changes
cg routes [filter]      # URL pattern -> handler
cg changes              # impact radius of uncommitted edits
```

All of the above accept `--json`. The graph auto-syncs via `cg watch`, or connect over MCP with `cg mcp-install`.
