# x402 v2 Conformance Vectors

The first-party x402 suite: every vector is a wire document of the x402 v2
message it names, together with whether the model must accept it.

## Files

- `payment-required.json` — `PaymentRequired` responses
- `payment-payload.json` — `PaymentPayload` requests
- `settlement-response.json` — `SettlementResponse` results

## Vector Format

Each file holds a JSON array of records:

- `name`: the case the vector covers
- `valid`: whether `parse` and `validate` must both succeed
- `document`: the wire document, exactly as it travels

## Conformance

`interop/crates/layerx-x402/tests/vectors.rs` reads these files and runs every
record through the production `PaymentRequired`, `PaymentPayload` and
`SettlementResponse` types, so the suite the deployment pins is the suite the
tests exercise. The vectors target the x402 v2 specification vendored at
`interop/specs/vendor/x402`.

`interop/deploy/gateway/render.py` derives the suite identifier, the vector
count and the SHA-256 the gateway configuration declares from these files.
