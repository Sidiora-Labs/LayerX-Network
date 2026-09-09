# 402LXP payments

402LXP pays for a resource with a signed LayerX receipt. An acknowledgement, queue position, submission identifier, or HTTP 200 is not payment. Settlement references are `lxp:<receipt_digest>`.

This page is the agent path for challenge, payer grant, settlement receipt, and renewal. The interop x402 adapter is [x402 transport](X402Transport.md). Gateway JSON-RPC is [Hosted gateway](HostedGateway.md). Protocol encodings are `spec/402lxp/protocol.md`.

## Flow

1. **Challenge.** The seller returns HTTP 402 with `PAYMENT-REQUIRED`: standard Base64 of UTF-8 JSON, at most 65,536 decoded bytes. The envelope is `x402Version: 2`, a `resource` URL, and one to 32 `accepts` entries. Each entry names `scheme` (`exact`, `metered`, or `subscription`), `network` (`layerx:<id>`), `asset` and `payTo` (64 lowercase hex), `amount` (canonical decimal, at most 2^128−1), and `maxTimeoutSeconds`.
2. **Terms.** `extra.layerx.commitment` is `executed`, `batched`, or `finalised`. Exact offers without `extra.layerx` default to `executed`. Grant schemes require an explicit commitment, `extra.layerx.purposeHash` (64 lowercase hex, not all zeros), and `extra.layerx.payer` (the grant `from` account). Subscriptions also require `extra.layerx.windowSeconds`.
3. **Payer grant.** Metered and subscription payments carry a canonical 346-byte payer grant inside a canonical 733-byte Asset receive (ordinal 6). The buyer authorizes the grant. The seller never manufactures payer authorization. A grant alone is not a payment.
4. **Submit.** The receiver signs the receive authorization preimage and an enclosing Asset activity, then submits `lx_sendActivity(canonical_hex, commitment)` at gateway `POST /rpc`. Request `executed`, then `batched` when the offer requires inclusion.
5. **Receipt.** `lx_getReceipt` and `lx_getActivityStatus` return `activity_id` and canonical receipt hexadecimal. A missing receipt is an RPC error. `state: "pending"` stays pending even when an executed receipt is present.
6. **Verify.** Check receipt signature against configured sequencer authority, then bind `amount`, `asset`, `payTo`, and `from` (the offer or grant payer). Grant draws also bind purpose. Pending or unknown results do not release the resource.
7. **Renewal.** A subscription is a recurring grant (`recurring = true`, positive `window_length` equal to `windowSeconds`). Each period is a new receive draw with a new idempotency key. Retries of one period reuse the exact signed activity. Query `lx_getReceipt` after an uncertain submit; do not replace the key.

## Public JSON-RPC

JSON-RPC 2.0 at `POST /rpc`. WebSocket notifications use `GET /rpc/ws` and `lx_subscribe` for `receipts`, `checkpoints`, or `account`. Treat notifications as triggers to fetch and verify evidence.

| Method | Role |
| --- | --- |
| `lx_sendActivity` | Submit canonical signed activity hex with commitment `executed`, `batched`, or `finalised` |
| `lx_getReceipt` | Canonical receipt for an activity |
| `lx_getActivityStatus` | Same receipt lookup contract as `lx_getReceipt` |
| `lx_getAccount`, `lx_getBalance`, `lx_getBalances` | Account and DID reads |
| `lx_getSequence` | Identity next sequence |
| `lx_estimateFee` | Fee estimate for canonical bytes |
| `lx_getBatchHeader`, `lx_getCheckpoint`, `lx_getProof` | Inclusion and finality evidence |
| `lx_listAssets`, `lx_getAsset`, `lx_getNodeInfo` | Registry and node reads |

JSON-RPC errors remain errors under HTTP 200. Trust roots are the verifier's configured network authority, not payment-header signatures.

## Encodings

Integers are big-endian. Native asset ids are `SHA-256("LX:ASSET:v1" \|\| issuer_did_id32 \|\| salt32)`. Per-asset accounts use `agent:<DID>:asset:<lowercase hex64 asset_id>` and the `LX:ACCOUNT:v1` id rule.

| Ordinal | Payload |
| --- | --- |
| 1 register | `version:u16=1` then asset id, salt, symbol, name, decimals, supply cap, `issuer_kind` (1 native, 2 `paxeer_custody`), custody ref |
| 4 account_open | `version:u16=1 \|\| asset_id32` |
| 6 receive | Canonical 733-byte receive, including the 346-byte payer grant |
| 7 grant_issue | Canonical grant |
| 8 grant_revoke | `version:u16=1 \|\| grant_id32 \|\| revocation_sequence:u64` |
| 9 | Reserved. Not defined |
| 10 mint / 11 burn | `version:u16=1 \|\| asset_id32 \|\| account_id32 \|\| amount:u128` |

`issuer_kind` is a wire namespace. Persisted `custody_kind` stays `LX_ASSET_CUSTODY_PAXEER = 1`. Kind-1 register payloads have empty custody refs and a derived asset id.

Python and TypeScript export `encode_register` / `encodeRegister`, receive and grant codecs, and `grant_payment_terms` / `grantPaymentTerms` from the public package entries (`layerx_sdk`, `@sidiora/layerx-sdk`, `@sidiora/layerx-seller-middleware`).
