# Public JSON-RPC

The public JSON-RPC gateway provides 15
`lx_*` methods: authenticated canonical submission, unauthenticated committed
reads, fee estimation, and scoped live subscriptions. The source contract is
the embedded OpenRPC 1.3.2 document plus the gateway dispatch and public-core
read implementations.

See [Commitment levels](CommitmentLevels.md), [Assets](Assets.md),
[Payments developer path](PaymentsQuickstart.md), and the
[exact real-process transcript](PublicAPI.md).

## Endpoints

| Transport | Endpoint | Use |
| --- | --- | --- |
| HTTPS | `https://api.testnet.layerx.network/rpc` | JSON-RPC 2.0 request or batch |
| HTTPS | `https://api.testnet.layerx.network/rpc/schema` | Embedded OpenRPC document |
| WebSocket | `wss://api.testnet.layerx.network/rpc/ws` | `lx_subscribe` |

`POST /rpc` requires `Content-Type: application/json`. It accepts a single
request or a batch of 1–32 requests. A request that carries no `id`, alone or
in a batch, produces no response body and returns HTTP 204; JSON-RPC responses
and errors use HTTP 200. The request body
limit enforced by the gateway is 8 MiB. Parameters are positional.

## Methods

| Method | Positional parameters | Result |
| --- | --- | --- |
| `lx_getAccount` | `[account_id]` | Authenticated account snapshot |
| `lx_getBalance` | `[account_id]` | The same account object; use its `balance` and `asset_id` |
| `lx_getBalances` | `[did]` | Complete bounded DID account list |
| `lx_getReceipt` | `[activity_id]` | `activity_id` and verified `receipt` bytes |
| `lx_getActivityStatus` | `[activity_id]` | The same receipt read, or an unavailable error while no receipt exists |
| `lx_getBatchHeader` | `[batch_number]` | Sequencer-signed batch header |
| `lx_getCheckpoint` | `[checkpoint_id]` | Checkpoint evidence |
| `lx_getNodeInfo` | `[]` or no `params` | Protocol/network handshake and current heads |
| `lx_getSequence` | `[account_id]` | The account object; read its `next_sequence` |
| `lx_getSequence` | `[did, "identity"]` | `did`, `next_sequence`, `observed_head_sequence`, `state_root`, and `verification: "authenticated_node_snapshot"` |
| `lx_getProof` | `["activity", activity_id]` | Activity proof and signed header |
| `lx_getProof` | `["receipt", activity_id]` | Receipt proof and signed header |
| `lx_getProof` | `["account", activity_id, account_id]` | Exact verified native account-proof bytes |
| `lx_sendActivity` | `[canonical_hex, commitment]` | Verified outcome at the requested commitment |
| `lx_subscribe` | `["receipts"]`, `["checkpoints"]`, or `["account", account_id]` | Subscription id string; WebSocket only |
| `lx_listAssets` | `[]` or no `params` | Asset registry snapshot under `assets`, bounded at 64 records |
| `lx_getAsset` | `[asset_id]` | One Asset metadata record |
| `lx_estimateFee` | `[canonical_hex]` | Committed-schedule estimate |

Although `lx_getSequence` has two parameter forms, it is one method; the table
therefore describes all 15 method names.

Identifiers are nonzero hexadecimal strings encoding 32 bytes. The gateway
selector accepts either case, but the receipt route refuses any uppercase
digit, so `lx_getReceipt` and `lx_getActivityStatus` require lowercase. Batch numbers are canonical nonzero decimal `u64` strings: `0` and
leading-zero forms are invalid. Canonical activity input is non-empty hex and
is limited to 512 KiB of decoded bytes.

## Read result objects

Public reads return native committed data; the gateway does not reconstruct
or invent proof material.

- An account includes `account_id`, `name`, `asset_id`, decimal-string
  `balance`, decimal-string `next_sequence`, `frozen`, `canonical_value`,
  `proof_material`, decimal-string `observed_head_sequence`,
  decimal-string `batch_number`, and `verification`. `verification` is one of
  `state_proven`, `checkpoint_finalised`, or `settlement_anchored`: the read is
  requested at state-proven level and fails closed if no level is achieved.
- A DID account list's entries carry the same fields without a per-entry
  `verification`; every entry still carries proof material that the client
  verifies against the signed batch header.
- A DID account list includes `did`, `accounts`, and
  `verification: "authenticated_node_snapshot"`. It is complete within the
  native bound; it does not independently prove completeness or finality.
- Asset results carry `asset_id`, `symbol`, `name`, `decimals`, `custody_kind`,
  `custody_reference`, `paused`, `supply_cap`, `issuer_did`, `issuer_kind`,
  `total_units`, and `salt`. A single record nests under `asset`; the list
  nests under `assets`. Both carry version-3 metadata, `observed_head_sequence`,
  `state_root`, and
  `verification: "authenticated_committed_snapshot"`. Amounts are decimal
  strings; ids and custody references are hexadecimal.
- A fee estimate includes decimal-string `fee`, `parameter_version`,
  hexadecimal `canonical_schedule`, `canonical_bytes`,
  `observed_head_sequence`, `state_root`, and
  `verification: "authenticated_committed_snapshot"`. It does not reserve a
  fee or prove execution.
- Node info includes `protocol_version`, `network_id`,
  `chain_head_sequence`, `latest_sealed_batch`,
  `latest_finalised_checkpoint`, `authorised_sequencer_key`, and
  `capabilities`.
- Proof results include `kind`, `activity_id`, `canonical_value`,
  `account_id` (null for activity and receipt kinds), `proof`, and a
  `signed_header` carrying `canonical_header`, `signature`, `sequencer_id`,
  `public_key`, and the covered batch interval. `proof` is polymorphic:
  activity and receipt proofs carry `leaf_index`, `leaf_count`, and
  `siblings`; an account proof carries `canonical_bytes`.
- Batch-header results carry `batch_number`, `canonical_header`, `signature`,
  `sequencer_id`, `sequencer_public_key`, `first_batch_number`, and
  `last_batch_number`. Note that a proof's signed header names the key
  `public_key` while the batch-header read names it `sequencer_public_key`.
- Checkpoint results carry `checkpoint_id`, `checkpoint`, `context`, and
  `canonical_header`.

Unsupported evidence, unknown records, malformed native responses, and
schedules requiring unavailable execution or storage inputs fail closed.

## Submit

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "lx_sendActivity",
  "params": ["<canonical activity hex>", "executed"]
}
```

`commitment` is exactly `executed`, `batched`, or `finalised`. The gateway
verifies the canonical envelope, actor authorization, protocol/network, and
registered module route before forwarding `application/octet-stream` with the
signed idempotency key. It preserves activity-id outer deduplication.

The admitted native operations are:

- Asset ordinals `1`, `4`, `5`, `6`, `7`, `8`, `10`, and `11`;
- Programs ordinals `1` deploy, `2` upgrade, `3` call, `5` transfer,
  `6` account registration, and `7` wind-down.

Asset pause/unpause (`2`/`3`) are excluded and Asset ordinal `9` is reserved.
Programs deploy, upgrade, call, and wind-down use their existing dedicated
gateway routes; other admitted activities use `/v1/activities`.

A successful result always contains a verified receipt and sets the exact
requested `commitment`. `batched` also contains `batch_evidence`; `finalised`
contains both `batch_evidence` and `checkpoint_evidence`. If the bounded wait
cannot establish the requested level, the response is error `-32001`
(`Requested commitment unavailable`) with `data.state = "pending"`,
`data.requested_commitment`, and `data.evidence` carrying what is already
available. When the upstream itself answers HTTP 202 the message is
`Submission unavailable` and the same `state`/`requested_commitment` pair is
returned with `data.upstream` instead of `data.evidence`. It never returns a
weaker success.

A success carries `activity_id`, `batch_id`, `global_sequence` (a JSON
number), `result_code`, `state_root`, `receipt`, `idempotency_key`, and
`commitment`.

## Authentication and scopes

Read methods require no credential. `lx_sendActivity` requires:

```text
Authorization: LayerX-Key <key-id>:lxp_live_<64-hex-secret>
```

The key must permit `activity:write`, and a Programs activity route must hold
`activity:write` **and** its own route scope. The full vocabulary is
`activity:write`, `program:call`, `program:simulate`, `state:read`,
`receipt:read`, and `program:read`. Missing or invalid authorization, a signer
mismatch, a disabled key record, and insufficient scope are refusals rather
than anonymous fallback. A key id must be non-empty, at most 64 characters, and
drawn from `[A-Za-z0-9-_]`.

## WebSocket subscriptions

Open `GET /rpc/ws` with WebSocket version 13, an empty body, no browser
`Origin`, and a `LayerX-Key` credential. Receipt subscriptions require
`receipt:read`; checkpoint and account subscriptions require `state:read`.
Sending `lx_subscribe` to HTTPS `POST /rpc` returns `-32004`.

The result of `lx_subscribe` is a one-based string id. Notifications are:

```json
{
  "jsonrpc": "2.0",
  "method": "lx_subscription",
  "params": {"subscription": "1", "result": {}}
}
```

Subscriptions are live signals, not durable replay. Reconnect and reconcile
with reads. The gateway allows 32 sockets, 8 subscriptions per socket, and 16
queued receipt wakes. It pings every 5 seconds, closes idle sockets after 60
seconds, and limits a connection to one hour; both of those expiries close with
`1000`. Slow consumers, feed loss, and exhausting the read rate limiter inside
a live socket close with `1013`; revoked keys or changed scope close with
`1008`; protocol errors close with `1002`.

Every read method is also servable over the socket: a frame whose `method` is
not `lx_subscribe` is dispatched as an ordinary JSON-RPC request, and the key
is re-authenticated on every inbound frame, every wake, and every ping.
`account` notifications fire only on a receipt wake and are suppressed when the
account is unchanged; the account id is lowercased into the topic name.
Checkpoint notifications also fire when a bounded wait expires.

The upgrade requires `Upgrade: websocket`, a `Connection` list containing
`upgrade`, a decodable `Sec-WebSocket-Key`, and a key already holding
`receipt:read` or `state:read`. Socket capacity is refused at the upgrade with
HTTP `429` and `Retry-After: 1`, never as a JSON-RPC error.

## JSON-RPC errors

| Code | Meaning |
| ---: | --- |
| `-32700` | Parse error |
| `-32600` | Invalid request envelope, empty batch, or batch over 32 |
| `-32601` | Method not found |
| `-32602` | Invalid positional parameters, selector, canonical activity, or commitment |
| `-32603` | Invalid upstream response, gateway persistence failure, invalid route, or missing verified receipt |
| `-32001` | Read/submission unavailable or requested commitment still pending |
| `-32002` | Authentication, authorization, or scope refusal |
| `-32004` | `lx_subscribe` requires WebSocket |
| `-32005` | Read rate limit, or the per-connection subscription limit |

For proxied calls, upstream HTTP `400`/`415` maps to `-32602`, `401`/`403`
to `-32002`, `429` to `-32005`, and other non-success status to `-32001`.
The upstream body is retained in `error.data`. Every proxied read carries the
message `Read unavailable` and every proxied submission `Submission
unavailable`, whichever of the four codes applies; gateway-side refusals carry
distinct messages such as `Insufficient scope`, `Invalid params`, and
`Missing verified receipt`.

The public read limiter admits 120 requests per second process-wide across all
read methods and refuses beyond that with HTTP `429 public_read_rate_limit` and
`Retry-After: 1`.

Non-JSON-RPC statuses on the public surface are `405 method_not_allowed` for a
non-POST `/rpc` or non-GET `/rpc/schema`, `415 json_content_type_required`,
`400 invalid_websocket_upgrade`, `403 insufficient_scope` at the WebSocket
upgrade, `401 api_key_required`, and `503 persistence_unavailable`. A request
carrying `x-layerx-principal` or `x-layerx-api-key` is refused with
`400 untrusted_identity_header`.

## Source contract

- `platform/hosted/gateway/openrpc.json` defines the public method contract.
- `platform/hosted/gateway/src/rpc.rs` dispatches HTTPS JSON-RPC.
- `platform/hosted/gateway/src/ws.rs` implements subscriptions.
- `platform/hosted/gateway/src/main.rs` owns routing (`route`), the scope table
  (`permits`), key authentication, and the request-size bound.
- `platform/hosted/gateway/src/lib.rs` owns canonical submission verification
  (`verify_submission`) and the key-id format bound.
- `platform/hosted/gateway/src/public_reads.rs` serves the unauthenticated REST
  read passthroughs `GET /v1/state`, `GET /v1/accounts/{id}/balance`, and
  `GET /v1/dids/{did}/accounts`, and holds the read limiter.
- `platform/hosted/core/src/public_reads.rs` implements authenticated native
  read translation.

[Home](Home.md)
