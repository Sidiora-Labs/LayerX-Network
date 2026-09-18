# x402 A2A Transport Conformance Vectors

The first-party x402 A2A binding suite: every vector is one role message as it
travels on this transport, together with whether the binding must accept that
representation.

## Files

- `role-messages.json` — one record per x402 role on this transport

## Vector Format

Each file holds a JSON array of records:

- `name`: the case the vector covers
- `transport`: the transport under test, so a record cannot be read by the
  wrong binding
- `role`: the x402 role the document plays
- `accepted`: whether the binding must accept this representation
- `representation`: `http-header` or `json`
- `header`: the exact header name a header representation travels in
- `document`: the wire document, exactly as it travels

This binding carries JSON on every message.

## Conformance

`interop/crates/layerx-x402/tests/transports.rs` reads these files with
`include_str!` and runs every record through the production x402 model types
and the production encoder and decoder for this transport, so the suite the
deployment pins is the suite the tests exercise.

The fault-injected settlement cases in that file are not vectors and are not
counted: they drive a live sequencer signer and a canonical receipt encoder
rather than a transport representation.

`interop/deploy/gateway/render.py` derives the vector count and the SHA-256
the gateway configuration declares from these files.

## LayerX Profile

The `layerx:*` requirements in these records carry the LayerX profile in
`extra.layerx` — `commitment`, the `agent:<did>:main` `account` a payer quotes
against, and the `CurrencyCode` of that quote — with `payTo` equal to the
account id derived from `account`. `PaymentRequirements::validate`
(`interop/crates/layerx-x402/src/model.rs`) enforces that binding on both
sides of this transport, so a representation advertising an account that does
not derive its `payTo` is refused before it is encoded or after it is decoded.
