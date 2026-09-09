# 402LXP payments

402LXP uses HTTP 402 offers and signed LayerX receipts to pay for resources in any registered asset. An acknowledgement, queue position, submission identifier, or successful HTTP response is not proof of payment. Settlement references remain `lxp:<receipt_digest>`.

## Offer

The `PAYMENT-REQUIRED` header is standard Base64 of UTF-8 JSON. Its decoded size must not exceed 65,536 bytes. The JSON envelope has `x402Version: 2`, a `resource` containing its URL, and between one and 32 `accepts` entries. Each entry contains:

| Field | Value |
| --- | --- |
| `scheme` | `exact`, `metered`, or `subscription` |
| `network` | `layerx:<network identifier>` |
| `asset` | Registered asset ID, 64 lowercase hexadecimal characters |
| `amount` | Positive base-unit price as a canonical decimal string, at most 2^128−1 |
| `payTo` | Recipient account ID, 64 lowercase hexadecimal characters |
| `maxTimeoutSeconds` | Positive integer, at most 2^32−1 |
| `extra.layerx.commitment` | `executed`, `batched`, or `finalised` |
| `extra.layerx.payer` | Required for grant draws; payer account ID as 64 lowercase hexadecimal characters |
| `extra.layerx.purposeHash` | Required for grant draws; 64 lowercase hexadecimal characters |
| `extra.layerx.windowSeconds` | Required only for subscriptions; positive canonical decimal u64 string |

All alternatives pay for the same resource. Each alternative independently identifies its asset, amount, recipient and commitment. The buyer selects a complete alternative and returns it unchanged as `accepted`. It must not substitute an asset, sum amounts across assets, infer an exchange rate, or replace the requested commitment with a weaker one. Registration and pause status are checked by the ledger when executing payment; an advertised asset ID is not proof of registration.

For an existing exact-payment offer without `extra.layerx`, the required commitment is `executed`. An invalid, unknown or incomplete `extra.layerx` value is refused. Grant schemes require explicit commitment, payer and purpose. Unknown schemes are refused.

## Payment header

`PAYMENT-SIGNATURE` uses the same Base64/UTF-8 JSON envelope limit. The envelope contains `x402Version: 2`, the unchanged `accepted` offer, the resource and required extensions, and a `payload`.

For `exact`, the payload contains the canonical receipt as standard Base64, `receiptDigest`, and `verificationLevel`. The receipt digest is SHA-256 over `LXP/v1/merkle-leaf` followed by one zero byte and the complete canonical receipt bytes. `verificationLevel` identifies the evidence supplied; it does not grant verification authority. An optional `idempotencyKey` identifies a retry of the same payment.

For `metered` and `subscription`, the payload contains `receive`, the canonical signed Asset receive payload as lowercase hexadecimal, and `idempotencyKey`, the receive's 32-byte key as 64 lowercase hexadecimal characters. The signed receive contains the payer grant. A grant alone is not a payment and never releases a resource. The receiver signs the canonical receive authorization preimage and submits an Asset activity with ordinal 6. The enclosing activity is signed by the receiver's identity and binds that identity's next sequence and fee limit. The receive payload's receiver sequence retains its native account-sequence meaning; it is not a substitute for the enclosing identity sequence.

Before submission, the receiver checks the selected payer, asset, amount, recipient, grant ID, purpose, reference and idempotency key against both the signed receive and its embedded grant. The buyer authorizes the grant; a seller must never manufacture payer authorization. The ledger verifies both signatures and all grant restrictions before debiting the payer. Malformed encodings, unauthorized or revoked grants, insufficient funds, exhausted allowances and expired grants are refusals, not alternative payment paths.

## Metered payments

Each request draws exactly its offered price against an issued payer grant. The grant binds the payer account, recipient account, asset, maximum per draw, total allowance, expiry, purpose, optional reference and revocation sequence. Nonrecurring grants have `recurring = false` and `window_length = 0`. A reference-bound invoice follows the ledger's single-settlement rule; it cannot be reused as a multi-request grant.

Persist the association between authenticated payer, resource request, selected offer, receive bytes, activity bytes, idempotency key and verified receipt. Concurrent retries of one request reuse the exact signed activity. A changed request requires a new authorization and key. A receipt cannot release a second distinct request. On a timeout or unknown submission result, retain the association and query activity status and receipt; do not submit a new draw with a new key. Only a verified successful receipt reaches fulfillment.

## Subscriptions

A subscription uses a recurring payer grant: `recurring = true`, positive `window_length`, and a per-window allowance. `window_length` must equal the offer's `windowSeconds`. Each renewal is an Asset receive draw, bounded by the per-draw maximum and the current window allowance. The ledger determines the window and enforces expiry, revocation and available balance. A local timer does not authorize a debit.

A subscription service persists its subscription identifier and period-to-request mapping. Retries within one period reuse that period's activity and idempotency key. A later period uses a new key and current sequences. The service must enforce one fulfillment per authorized subscription period. It must not claim that the recurring allowance itself enforces one renewal per period: a grant may authorize multiple draws within its allowance.

## Commitment verification

| Requested commitment | Required evidence |
| --- | --- |
| `executed` | Canonical successful receipt with a valid signature from the authorized sequencer and matching payment facts |
| `batched` | Executed evidence plus receipt Merkle inclusion in a signed, authorized batch header |
| `finalised` | Batched evidence plus the guarantor checkpoint certificate covering that batch |

Trust inputs come from the verifier's configured network authority, not from the payment header. Verify the receipt's asset, amount, payer, recipient, successful result and expected activity binding. Bind grant payments to the particular receive activity and request. Check the signed batch header's network, protocol, sequence coverage and authorization. Check the checkpoint identity, bonded guarantor set, required threshold, signatures, settlement domain and data-availability requirements. A certificate for a different batch does not promote a receipt's commitment. Missing or invalid evidence never downgrades a request to `executed`.

Return `PAYMENT-RESPONSE` only after the selected commitment has been verified. A successful response contains `success: true`, the selected `network` and `amount`, `payer`, `transaction: "lxp:<receipt_digest>"`, and the receipt evidence. Grant settlements also repeat the challenge's `purposeHash` in `extensions.layerx`. Buyers reject a settlement unless its payer is the verified receipt source and, for grant draws, its payer and purpose exactly match the challenge. Pending and unknown results are not success and must not release the resource. A refusal has `success: false`, an error reason, and no successful settlement reference. Buyers independently verify returned evidence and bind the settlement reference to their payment.

## Canonical grant and receive

All integers are unsigned and big-endian. Every ID and hash is exactly 32 bytes; Ed25519 signatures are exactly 64 bytes. Boolean fields are one byte, either zero or one. No field is omitted. Trailing bytes are refused.

The canonical grant is the concatenation of:

`grant_id32 || from32 || recipient32 || asset32 || per_draw_maximum:u128 || allowance:u128 || recurring:u8 || window_length:u64 || expiration:u64 || purpose_hash32 || has_reference:u8 || reference_hash32 || revocation_sequence:u64 || public_key32 || signature64`.

The grant authorization message is ASCII `LXP:GRANT:v1` followed by the grant fields from `from` through `public_key`, excluding `grant_id` and `signature`. Grant identity and signature use the native authority-hash domain.

The canonical receive is 733 bytes:

`tag:u16=0x5201 || field_count:u16=10 || from32 || to32 || asset32 || amount:u128 || grant_id32 || receiver_sequence:u64 || idempotency_key32 || context_hash32 || receiver_authorization || payer_grant`.

Receiver authorization is:

`kind:u8 || controller32 || public_key32 || signature64 || signed_context_hash32 || network_id:u32 || protocol_version:u16`.

The receive authorization message is ASCII `LXP:RECEIVE:v1`, followed by the receive fields from `from` through `context_hash`, followed by receiver authorization without its public key or signature. The receiver signs using the native signature-preimage domain. These payload encodings do not replace the signed activity envelope.

Asset grant issue uses ordinal 7 with the canonical grant. Grant revoke uses ordinal 8 and `version:u16=1 || grant_id32 || revocation_sequence:u64`. Ordinal 9 is reserved. Python and TypeScript codecs for these payloads live in `agent/sdk/python/layerx_sdk/x402_receive.py` and `agent/sdk/typescript/src/x402/receive.ts`.

## Asset accounts

The per-asset account name is `agent:<DID>:asset:<lowercase hex64 asset_id>`. Its account ID follows `SHA-256("LX:ACCOUNT:v1" || name_byte_length:u32 || UTF-8(name))`. The native account remains `agent:<DID>:main`. Open the registered, unpaused asset account through Asset ordinal 4 with `version:u16=1 || asset_id32`.

Natively issued asset IDs are `SHA-256("LX:ASSET:v1" || issuer_did_id32 || salt32)`. Custody asset IDs retain their registered values. Asset symbols and display names are not identifiers.

Asset register uses ordinal 1 and `version:u16=1 || asset_id32 || salt32 || symbol_len:u8 || symbol(1..16 ASCII) || name_len:u8 || name(1..32 UTF-8) || decimals:u8(<=38) || supply_cap:u128 (0 = uncapped) || issuer_kind:u8 (1 native, 2 paxeer_custody) || custody_ref_len:u8 || custody_ref(<=128)`. Issuer is the actor. For kind 1 the asset ID must match the derivation and the custody reference must be empty. Duplicates are refused.

Asset mint uses ordinal 10 and `version:u16=1 || asset_id32 || to_account32 || amount:u128`. The actor must be the asset issuer, amount must be positive, `total_units + amount` must not exceed a nonzero supply cap, and the destination account must exist for that asset.

Asset burn uses ordinal 11 and `version:u16=1 || asset_id32 || from_account32 || amount:u128`. The actor must own `from_account`, amount must be positive, and the account must have sufficient balance.

Every new activity binds its sequence to `identity.next_sequence` like Send, charges the existing fee schedule, and produces a receipt that binds the before and after balances and `total_units`.

## Public transport

Use JSON-RPC 2.0 at gateway `POST /rpc`. The read methods are `lx_getAccount`, `lx_getBalance`, `lx_getBalances(did)`, `lx_getSequence`, `lx_estimateFee`, `lx_getReceipt`, `lx_getActivityStatus`, `lx_getBatchHeader`, `lx_getCheckpoint`, `lx_getProof`, `lx_listAssets`, `lx_getAsset` and `lx_getNodeInfo`. Submit canonical signed activity hexadecimal with `lx_sendActivity(canonical_hex, commitment)`.

WebSocket subscriptions use `GET /rpc/ws` and `lx_subscribe` for `receipts`, `checkpoints` or `account`. Treat notifications as triggers to obtain and verify evidence. JSON-RPC errors remain errors even when transported with HTTP 200. A submit acknowledgement is never reported as payment success. Faucet funding is a separate operation and must itself be confirmed before assuming that funds are spendable.

### RPC result fields

`lx_getReceipt(activity_id)` and `lx_getActivityStatus(activity_id)` return `activity_id` and `receipt`, where `receipt` is canonical receipt hexadecimal. Status lookup uses the receipt lookup contract. A missing receipt is an RPC error, not successful execution.

`lx_sendActivity` returns `activity_id`, `receipt` when available, and the established `commitment`. A result with `state: "pending"` remains pending even if it contains an executed receipt. Verify the expected activity ID and payer as well as asset, amount and recipient. Do not release a resource on an admission acknowledgement.

The grant flow is challenge → payer grant → settlement receipt → renewal. The seller emits a metered or subscription challenge containing payer, purpose and commitment. The payer signs and issues the ordinal-7 grant, and the receiver prepares an ordinal-6 draw. Submit the signed draw through `lx_sendActivity(canonical_hex, "executed")` for an executed challenge or `lx_sendActivity(canonical_hex, "batched")` for a batched challenge. If submission returns JSON-RPC code `-32001` with `state: "pending"`, retain the exact signed activity and idempotency key. Recover its receipt with `lx_getActivityStatus(activity_id)` or `lx_getReceipt(activity_id)`, verify the sequencer signature and successful result, and verify that the receipt names the submitted activity. A recovered receipt establishes only `executed`. For `batched`, also obtain `lx_getProof("receipt", activity_id)`, match its activity and canonical value to the receipt, and verify inclusion in the authorized signed batch header. Never convert the pending response itself into success or issue a replacement debit. A renewal is a new signed ordinal-6 activity using the same recurring grant, a new current sequence and a new period idempotency key. Only the receipt and evidence satisfying the challenged commitment authorize each settlement.

For batched results, `batch_evidence` contains `kind: "receipt"`, `activity_id`, `canonical_value`, `proof` (`leaf_index`, `leaf_count`, `siblings`), and `signed_header` (`canonical_header`, `signature`, `sequencer_id`, `public_key`, `first_batch_number`, `last_batch_number`). Match the canonical value to the receipt before checking inclusion. Sequencer authorization and network identity come from configured trust inputs.

For finalised results, `checkpoint_evidence` contains `checkpoint_id`, `checkpoint`, `context`, and `canonical_header`. These are canonical evidence bytes. Their presence alone does not establish finality; a checkpoint verifier must validate them against configured guarantor authority and the exact batch header.

### Running the public examples

Build the TypeScript SDK and seller middleware, then run:

```sh
node platform/middleware/examples/public-rpc.mjs --help
PYTHONPATH=agent/sdk/python python3 platform/middleware/examples/public_rpc.py --help
```

Supply the gateway `/rpc` URL and your DID explicitly. Optional faucet claims require the faucet `/v1/faucet/claims` URL, public key and a stable claim key. Authentication uses `LAYERX_RPC_TOKEN` and `LAYERX_FAUCET_TOKEN`. Do not change a claim key after an uncertain result.

To submit a payment, provide a signed canonical activity hex file, a Base64 `PAYMENT-REQUIRED` header file, the payer account ID and a trusted authority JSON file. For executed commitment, the authority file contains hexadecimal `batchId`, `asset`, `previousStateRoot`, `resultingStateRoot` and `sequencerPublicKey`. A batched offer additionally requires numeric `networkId`, hexadecimal `sequencerId`, and canonical decimal `firstBatchNumber` and `lastBatchNumber`. Obtain all authority fields from configured trust, never from returned evidence. The examples select an executed or batched offer, verify the signed receipt and requested commitment, and print the `lxp:` settlement reference. They report pending with exit code 2. Python payment verification additionally requires the `cryptography` package.

A faucet HTTP success does not independently prove a spendable balance. Read and verify the funded account before preparing a payment activity. Prepare activities with current identity sequences; retain the same signed bytes for uncertain-result recovery.

### Prepared grant draws

The Node SDK `PreparedGrantDraws` and Python `PreparedGrantDraws` persist signed receive activities in SQLite. Register the authenticated principal, seller request digest, canonical signed activity, canonical receive payload and its idempotency key before accepting a draw. Subscription registrations additionally require a stable, unique subscription-period key. Registration rejects an existing key with different request ownership or signed activity.

Compose the Node executor with `GrantPaymentAuthority` and the normal seller receipt verifier; configure commitment resolution for batched or finalised offers. Python supplies the executor as the seller's `draw` callable. The executor binds Asset ordinal 6, receiver DID, network, payload hash and idempotency key, and verifies returned payment facts before returning receipt evidence. The ledger remains responsible for signature authority, current sequence, fees, grant revocation, allowance and asset-state checks.

An attempt is persisted before submission. After an uncertain result, retrying the request performs `lx_getReceipt` for the same activity ID rather than creating another debit. Missing evidence stays pending. A crash before the first network write can therefore leave a registered request pending; reconcile that exact activity before taking further action. Never replace its key to bypass uncertainty.

Buyers use `grantHeader`/`grant_header` to preserve the selected offer and signed receive bytes, then `captureGrantSettlement`/`capture_grant_settlement` with the expected enclosing activity ID. The HTTP helpers verify settlement before returning paid content. Grant payments do not enter the quote-and-transfer path.

### Python agent and example configuration

Python `x402_agent.AgentGrantMiddleware` wraps `PreparedGrantDraws` for the seller's
`draw` argument. Configure `PaymentBudget` with a SQLite path, tenant, asset, and
explicit integer allowance. Reservations bind the request digest and amount; unknown
submissions retain the reservation. Only a receipt satisfying the offer commitment
can commit it. Replays reuse the reservation and a receipt cannot settle another
reservation. Configuration changes to a persisted allowance are refused.

Both public examples read `LAYERX_RPC_URL`, `LAYERX_FAUCET_URL`, and `LAYERX_DID`;
arguments can override these values. Authentication uses `LAYERX_RPC_TOKEN` and
`LAYERX_FAUCET_TOKEN`. The examples refuse port 18545. Signed activity files can pay
exact, metered, or subscription offers at executed commitment; the buyer and seller
middleware also support batched and finalised commitments through configured evidence.
