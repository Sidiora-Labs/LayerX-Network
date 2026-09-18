# UCP Checkout Conformance Vectors

The first-party UCP suite: every vector is a wire value of the UCP checkout
surface it names, together with whether the production type must accept it.

## Files

- `capabilities.json` — capability names, versions, specification and schema URLs
- `checkout-status.json` — the checkout status tokens and what each one means
- `idempotency-keys.json` — the exact idempotency-key strings and their refusals
- `merchant-profiles.json` — merchant profile documents and their refusals
- `payment-handlers.json` — payment handler declarations and digest grouping

## Vector Format

Each file holds a JSON array of records:

- `name`: the case the vector covers
- `valid`: whether the production constructor must succeed
- `refusal`: the exact `UcpError` variant an invalid record must produce
- the remaining fields are the wire values of that record, exactly as they
  travel: `capability`, `version`, `spec`, `schema`, `rest_endpoint`,
  `payment_handler`, `key`, `status`, `id`
- `digest_group` (payment handlers): records sharing a group must produce the
  same payment handler digest, and records in different groups must not

## Conformance

`interop/crates/layerx-ucp/tests/conformance_vectors.rs` reads these files with
`include_str!` and runs every record through the production `MerchantProfile`,
`PaymentHandler`, `IdempotencyKey`, `CheckoutStatus` and `UcpCapability` types,
so the suite the deployment pins is the suite the tests exercise.

Two cases in that file are not vectors and are not counted: the Codify anchor
and the vendored-revision check assert against constants compiled into the
crate rather than against wire data.

`interop/deploy/gateway/render.py` derives the suite identifier, the vector
count and the SHA-256 the gateway configuration declares from these files.
