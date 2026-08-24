# LayerX market-maker ramp toolkit

This package is for an independent market maker operating ordinary LayerX agent accounts. It creates no protocol role, minting power, settlement authority, reserved vocabulary or LayerX custody claim. Every customer surface must display:

> External custody: this independent market maker controls the off-platform funds and payout.

Paxeer remains the sole LayerX custody and guaranteed-withdrawal boundary. The reference service uses Paxeer only to rebalance the operator's configured inventory account; a customer order can never select that account, wallet, vault, signer or custody operation.

## Order and money binding

`RampOrder::bind` combines an authenticated identity-plane customer with an operator-owned quote and operator configuration. The request supplies only `order_id`, `quote_id` and the payer-grant identifier required for an off-ramp; an on-ramp rejects a payer grant because its LayerX leg is the operator's direct send. A domain-separated `OrderDigest` binds direction, order and quote identities, authenticated customer and operator principals/accounts, the operator's non-exporting signer handle, LayerX asset and exact integer amount, external currency and exact minor-unit amount, rational rate, fees, slippage bound, provider and payout tokens, expiry, payer grant presence/value and the operator-owned application context. That digest is also the LayerX idempotency key, and the bound context is required in the receipt.

The ordinary-principal LayerX path compiles the existing typed `LxpSend` for the operator-to-customer on-ramp leg and `LxpReceive` under the authenticated customer's payer grant for the customer-to-operator off-ramp leg. The on-ramp send authorization and both canonical envelopes are signed by the operator's remote non-exporting mTLS signer under the declared protocol version, network and LayerX signature-preimage domain, then verified locally. The service submits the exact signed bytes to the hosted gateway. Unknown submission is retained with those exact bytes and resolved only by activity receipt lookup. A terminal LayerX leg requires an independently fetched `AuthorizedBatch` and local verification of the receipt signature, activity ID, debit/credit accounts, asset, amount and order context.

## Durable workflow

The journal is append-only, hash-chained, mode `0600`, protected by a kernel-released exclusive writer lock, and fsyncs every event before changing in-memory state. Startup replays and authenticates the whole chain. Financial side effects are preceded by a durable `*_submission_planned` transition. Worker leases, order IDs, digest idempotency, provider callback IDs/sequences, exact signed LayerX activities, Paxeer operation IDs and transaction hashes are persisted. Planned and submitted-unknown operations are reconciled by the same provider idempotency key or LayerX activity ID and are never blindly resubmitted.

The public status vocabulary is `pending`, `unknown`, `refused`, `manual_review`, `reversed` and `done`. `done` requires a verified LayerX receipt and, for off-ramp, a settled external payout. A later provider reversal removes `done` immediately. Provider, compliance, LayerX and Paxeer failures are never translated to a safe outcome.

## Production boundaries

All egress except the existing Paxeer JSON-RPC reader is pinned HTTPS with mutual TLS. Credentials are bounded mode-`0600` secret files. The service persists only KMS/HSM handles and public keys; it never accepts or stores signing material. The strict deploy-supplied contracts are:

- `layerx-ramp-compliance-v1`: signed, expiring, order/customer/operator-bound decisions owned by the operator compliance service.
- `layerx-ramp-provider-v1`: idempotent settlement submission/status and signed callbacks bound to customer, order, beneficiary, exact amount and currency.
- the production KMS signature boundary: `POST /v1/signatures` with the non-exporting `key_handle`, `algorithm: ed25519` and the standard-padded-base64 LayerX domain digest, returning only a standard-padded-base64 signature which is verified against the configured public key.
- `layerx-ramp-paxeer-v1`: operator-only broadcast using the configured account, wallet, vault and non-exporting signer handle.

The repository does not invent provider, compliance or Paxeer custody-owner wire coordinates. Deployments must supply the versioned paths and owner-published schemas described in `OPERATIONS.md` and must qualify those contracts against real sandboxes.

## Run

Copy `config.example.json` outside the repository, replace every explicit deployment coordinate, mount the referenced secret files with mode `0600`, and run:

```sh
cargo run --locked --manifest-path platform/Cargo.toml -p layerx-reference-ramp -- /run/layerx-ramp/config.json
```

The executable serves TLS on the configured address. Public routes are `POST /v1/orders`, `GET /v1/orders/{order_digest}`, `POST /v1/provider-callbacks`, `/livez` and `/readyz`. The NetworkPolicy-restricted operator worker routes are `POST /internal/v1/work`, `POST /internal/v1/rebalances` and `GET /internal/v1/rebalances/{idempotency_key}`. Customer identity comes only from remote token introspection; customer JSON cannot set a customer account, operator account, compliance result, provider credential, signer handle or activity ID.
