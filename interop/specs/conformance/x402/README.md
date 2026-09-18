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

## LayerX Profile

Vectors on a `layerx:*` network follow the LayerX profile the buyer and seller
middleware speak: `extra.layerx` carries the `commitment`, the
`agent:<did>:main` `account` the payer quotes against, and the `CurrencyCode`
that quote is priced in, and `payTo` is the account id derived from that
reference under `SHA256("LXP/v1/account-id\0" || name)` or
`SHA256("LX:ACCOUNT:v1" || u32be(len) || name)`. The suite therefore holds
refusals for an offer that omits the block, one whose `payTo` is another
account's identifier, and one whose currency is not a currency code, plus an
accepted offer outside `layerx:*` that carries no such block. The rule lives in
`PaymentRequirements::validate` (`interop/crates/layerx-x402/src/model.rs`), so
it holds for every message that embeds requirements.
