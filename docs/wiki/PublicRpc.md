# Public JSON-RPC

The public JSON-RPC gateway is on the testnet branch. It provides 15
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
request or a batch of 1–32 requests. A batch containing only notifications
returns HTTP 204; JSON-RPC responses and errors use HTTP 200. The request body
limit enforced by the gateway is 8 MiB. Parameters are positional.

## Methods

| Method | Positional parameters | Result |
| --- | --- | --- |
| `lx_getAccount` | `[account_id]` | Authenticated account snapshot |
| `lx_getBalance` | `[account_id]` | The same account object; use its `balance` and `asset_id` |
| `lx_getBalances` | `[did]` | Complete bounded DID account list |
| `lx_getReceipt` | `[activity_id]` | Verified receipt and result code |
| `lx_getActivityStatus` | `[activity_id]` | The same receipt read, with completed/refused state or an unavailable error |
| `lx_getBatchHeader` | `[batch_number]` | Sequencer-signed batch header |
| `lx_getCheckpoint` | `[checkpoint_id]` | Checkpoint evidence |
| `lx_getNodeInfo` | `[]` or no `params` | Protocol/network handshake and current heads |
| `lx_getSequence` | `[account_id]` | Account `next_sequence` |
| `lx_getSequence` | `[did, "identity"]` | Independent identity `next_sequence` for envelope signing |
| `lx_getProof` | `["activity", activity_id]` | Activity proof and signed header |
| `lx_getProof` | `["receipt", activity_id]` | Receipt proof and signed header |
| `lx_getProof` | `["account", activity_id, account_id]` | Exact verified native account-proof bytes |
| `lx_sendActivity` | `[canonical_hex, commitment]` | Verified outcome at the requested commitment |
| `lx_subscribe` | `["receipts"]`, `["checkpoints"]`, or `["account", account_id]` | Subscription id string; WebSocket only |
| `lx_listAssets` | `[]` or no `params` | Complete Asset registry snapshot |
| `lx_getAsset` | `[asset_id]` | One Asset metadata record |
| `lx_estimateFee` | `[canonical_hex]` | Committed-schedule estimate |

Although `lx_getSequence` has two parameter forms, it is one method; the table
therefore describes all 15 method names.

Identifiers are nonzero lowercase or uppercase hexadecimal strings encoding
32 bytes. Batch numbers are canonical nonzero decimal `u64` strings: `0` and
leading-zero forms are invalid. Canonical activity input is non-empty hex and
is limited to 512 KiB of decoded bytes.

## Read result objects

Public reads return native committed data; the gateway does not reconstruct
or invent proof material.

- An account includes `account_id`, `name`, `asset_id`, decimal-string
  `balance`, decimal-string `next_sequence`, `frozen`, `canonical_value`,
  `proof_material`, decimal-string `observed_head_sequence`, and
  decimal-string `batch_number`.
- A DID account list includes `did`, `accounts`, and
  `verification: "authenticated_node_snapshot"`. It is complete within the
  native bound; it does not independently prove completeness or finality.
- Asset list/detail results carry version-3 metadata, `observed_head_sequence`,
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
- Proof results include the proof `kind`, `activity_id`, `canonical_value`,
  proof material, and a `signed_header` with its canonical header, signature,
  sequencer identity/key, and covered batch interval.

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
cannot establish the requested level, the response is error `-32001` with
`data.state = "pending"`, `data.requested_commitment`, and the evidence already
available. It never returns a weaker success.

## Authentication and scopes

Read methods require no credential. `lx_sendActivity` requires:

```text
Authorization: LayerX-Key <key-id>:lxp_live_<64-hex-secret>
```

The key must permit `activity:write`, and Programs activity routes must also
pass their existing route scope. Missing or invalid authorization, a signer
mismatch, and insufficient scope are refusals rather than anonymous fallback.

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
seconds, and limits a connection to one hour. Slow consumers or feed loss close
with `1013`; revoked keys or changed scope close with `1008`; protocol errors
close with `1002`.

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
| `-32005` | Read rate limit or WebSocket/subscription capacity |

For proxied calls, upstream HTTP `400`/`415` maps to `-32602`, `401`/`403`
to `-32002`, `429` to `-32005`, and other non-success status to `-32001`.
The upstream body is retained in `error.data`.

## Source contract

On the testnet branch:

- `platform/hosted/gateway/openrpc.json` defines the public method contract.
- `platform/hosted/gateway/src/rpc.rs` dispatches HTTPS JSON-RPC.
- `platform/hosted/gateway/src/ws.rs` implements subscriptions.
- `platform/hosted/gateway/src/lib.rs` owns routing, authorization, and
  canonical submission verification.
- `platform/hosted/core/src/public_reads.rs` implements authenticated native
  read translation.

[Home](Home.md)
