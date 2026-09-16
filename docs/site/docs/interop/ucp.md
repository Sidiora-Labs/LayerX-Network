# UCP

`layerx-ucp` is the interoperability adapter for UCP merchant profiles,
checkout, and orders (`interop/crates/layerx-ucp`). The adapter id is `ucp`.
The crate pins UCP revision `2026-04-08` and the checkout / order
specification and schema URLs published at `ucp.dev`
(`interop/crates/layerx-ucp/src/lib.rs`).

The platform specification (`spec/layerx-platform/spec.kvx`, requirement 32
`ac_4`) requires UCP merchant profiles, capability negotiation, checkout, and
order flows so a UCP-speaking commerce surface can transact against LayerX
sellers without bespoke integration.

## What the crate binds

Capability names compiled in the crate include
`dev.ucp.shopping.checkout` and `dev.ucp.shopping.order`. Settlement
success still requires a gateway-verified LayerX receipt: the crate imports
`layerx_proof::receipt::verify` and `AuthorizedBatch`. Domain tags
`LayerX/interop/ucp/checkout/v1` and `LayerX/interop/ucp/idempotency/v1`
are defined in the same file.

Adapters translate. They do not write balances
([Interop](index.md)).

## What this page does not document

No public hosted UCP origin is named in `docs/wiki`. This page does not
invent one. Merchant-profile and checkout HTTP paths are those of the
pinned UCP revision, spoken by the adapter, not additional LayerX RPC
methods.

## Status

The adapter crate, pinned vendor documents under
`interop/specs/vendor/ucp/`, and tests under
`interop/crates/layerx-ucp/tests/` are in this tree. End-to-end
qualification against the published UCP vectors is a platform / beta gate
and is not claimed here.
