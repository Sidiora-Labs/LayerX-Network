# Public JSON-RPC

Checked against `platform/hosted/gateway/openrpc.json` on **the testnet
branch** `lane/pay-public-rpc` (OpenRPC 1.3.2, API version 0.1.0). That
file is not on `main`. `main` has no `/rpc`, `/rpc/schema`, or `/rpc/ws`
route (`platform/hosted/gateway/src/main.rs` on `main`).

Title: *LayerX public JSON-RPC*. Description from the document:

> Public JSON-RPC reads, authenticated activity submission and scoped
> WebSocket subscriptions. Native DID enumeration, asset listing/detail
> and fee estimation are unavailable on this source; their gateway
> methods forward to core and return explicit upstream unavailability,
> never fabricated balances, metadata or fees.

Related pages: [Commitment levels](CommitmentLevels.md),
[Assets](Assets.md), [Hosted gateway](HostedGateway.md),
[Payments developer path](PaymentsQuickstart.md).

---

## Endpoints

| Method | Path | Role |
| --- | --- | --- |
| `POST` | `/rpc` | JSON-RPC 2.0, single object or batch (max 32) |
| `GET` | `/rpc/schema` | Embedded OpenRPC document |
| `GET` | `/rpc/ws` | WebSocket upgrade for `lx_subscribe` |

Hosted testnet ingress on that branch remains
`api.testnet.layerx.network` (`platform/hosted/gateway/deployment.yaml`).
`POST /rpc` requires `Content-Type: application/json`. Parse errors
return HTTP 200 with JSON-RPC `-32700`. A notification-only batch
returns HTTP 204.

`lx_subscribe` over HTTP POST is `-32004` (WebSocket required).

---

## Methods

Params are positional (`paramStructure: by-position`).

### Reads

| Method | Params | Result name | Documented errors |
| --- | --- | --- | --- |
| `lx_getAccount` | `account_id` (string) | `result` | `-32001`, `-32005` Read unavailable |
| `lx_getBalance` | `account_id` (string) | `result` | `-32001`, `-32005` |
| `lx_getBalances` | `did` (string) | `result` | `-32001`, `-32005` |
| `lx_getSequence` | `account_id` (64 hex) | `snapshot` | *(none in OpenRPC)* |
| `lx_getReceipt` | `activity_id` (string) | `result` | `-32001`, `-32005` |
| `lx_getActivityStatus` | `activity_id` (string) | `result` | `-32001`, `-32005` |
| `lx_getBatchHeader` | `batch_number` (string) | `result` | `-32001`, `-32005` |
| `lx_getCheckpoint` | `checkpoint_id` (string) | `result` | `-32001`, `-32005` |
| `lx_getProof` | `kind` (`activity` \| `receipt` \| `account`), `activity_id` (64 hex), optional `account_id` (64 hex; required when `kind` is `account`) | `proof` | *(none in OpenRPC)* |
| `lx_getNodeInfo` | *(none)* | `result` | `-32001`, `-32005` |
| `lx_listAssets` | *(none)* | `result` | `-32602`, `-32001`, `-32005` |
| `lx_getAsset` | `asset_id` (64 hex) | `result` | `-32602`, `-32001`, `-32005` |
| `lx_estimateFee` | `canonical_hex` (hex pairs, OpenRPC `maxLength` 1048576) | `result` | `-32602`, `-32001`, `-32005` |

OpenRPC notes:

- `lx_getSequence`: *"Source account next_sequence from the authenticated
  node account read. The envelope uses the separate identity sequence."*
- `lx_getProof`: *"Verified proof with canonical value and signed batch
  header. Account proofs export the exact verified native proof bytes
  without reconstruction."*
- `lx_listAssets`: *"Lists assets through the core read contract. Native
  integration remains required."*
- `lx_getAsset`: *"Reads native asset metadata. Native integration remains
  required."*
- `lx_estimateFee`: *"Forwards canonical activity bytes to the core
  fee-estimation contract. Fee values must come from the native fee
  schedule; native integration remains required."*

On that branch, `lx_getBalances`, `lx_listAssets`, `lx_getAsset`, and
`lx_estimateFee` forward to core paths that return unavailability
(`-32001`). They do not invent balances, metadata, or fees.

`lx_getActivityStatus` is dispatched to the same receipt read as
`lx_getReceipt` (`platform/hosted/gateway/src/rpc.rs` on that branch).

Unauthenticated reads are allowed. Public reads are rate-limited
(120/s per instance on that branch).

### Submit

| Method | Params | Result name |
| --- | --- | --- |
| `lx_sendActivity` | `canonical_hex` (hex pairs, OpenRPC `maxLength` 1048576), `commitment` (`executed` \| `batched` \| `finalised`) | `outcome` |

OpenRPC description:

> Requires an activity:write gateway key and the existing route scope
> for Programs activities. Uses activity-ID outer deduplication and
> preserves signed protocol idempotency. Returns a verified receipt for
> executed, verified inclusion for batched, and an exact matching
> verified checkpoint header for finalised; returns pending when
> evidence is absent after the bounded wait. An admission
> acknowledgement never establishes execution.

`commitment` `"ack"` is invalid (`-32602`). See
[Commitment levels](CommitmentLevels.md).

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "lx_sendActivity",
  "params": ["<canonical activity hex>", "executed"]
}
```

### Subscribe

| Method | Params | Result name |
| --- | --- | --- |
| `lx_subscribe` | `topic` (`receipts` \| `checkpoints` \| `account`), optional `account_id` (64 hex; required for `account`) | `subscription` (string) |

OpenRPC description (abridged to the document's constraints):

> Authenticated `GET /rpc/ws` upgrade using `Authorization: LayerX-Key`.
> `receipt:read` for receipts; `state:read` for checkpoints and account.
> Server notifications use `lx_subscription` with `params.subscription`
> and `params.result`. At most 32 connections, 8 subscriptions per
> connection and 16 queued receipt wakes. Slow consumers and feed loss
> close with 1013; key revocation closes with 1008. Reconnect and
> reconcile through reads after closure; notifications are live, not a
> durable replay. Ping every 5 seconds, idle deadline 60 seconds,
> connection lifetime one hour. Browser `Origin` requests are refused.

---

## Errors

JSON-RPC codes. HTTP 200 may still carry an error object. Never treat
an ack as success.

| Code | OpenRPC / wire message | Typical cause on that branch |
| ---: | --- | --- |
| `-32700` | Parse error | Invalid JSON |
| `-32600` | Invalid Request | Bad envelope; empty or oversized batch |
| `-32601` | Method not found | Unknown method |
| `-32602` | Invalid params | Bad params, bad activity, or `"ack"` commitment |
| `-32603` | Invalid upstream response / Missing verified receipt | Upstream JSON miss; HTTP 200 without a verified receipt |
| `-32001` | Read unavailable / Submission unavailable | Core non-success (except 429/401/403) |
| `-32002` | Authentication required / Insufficient scope | Missing key or wrong scope |
| `-32004` | WebSocket required | `lx_subscribe` over HTTP POST |
| `-32005` | Read unavailable / Subscription limit | Rate limit or WS capacity |

---

## Auth

| Call | Credential |
| --- | --- |
| Read methods | none |
| `lx_sendActivity` | `Authorization: LayerX-Key {id}:{secret}` with `activity:write` (and `program:call` for Programs ordinals 1/2/3/7) |
| `GET /rpc/ws` | `LayerX-Key` with `receipt:read` or `state:read` |

Key format matches the hosted gateway on `main`
(`platform/hosted/gateway/src/lib.rs`).

---

## Schema source

On `lane/pay-public-rpc`:

- Document: `platform/hosted/gateway/openrpc.json`
- HTTP dispatch: `platform/hosted/gateway/src/rpc.rs`
- WebSocket: `platform/hosted/gateway/src/ws.rs`, `ws_wire.rs`
- `GET /rpc/schema` serves the embedded document

The document leaves generic results as `"type": "object"`; it does not
define additional result fields.

[Home](Home.md)
