# UCP

`layerx-ucp` implements UCP revision `2026-04-08`: merchant profile,
capability negotiation, checkout completion, and order read backed by
verified LayerX receipts (`interop/crates/layerx-ucp/src/lib.rs`).
The adapter id is `ucp`. `spec/layerx-platform/spec.kvx` task 23.2 is
**done**.

---

## Protocol constants in code

- `UCP_VERSION`
- Capabilities `dev.ucp.shopping.checkout` and
  `dev.ucp.shopping.order`
- Profile URLs under `https://ucp.dev/2026-04-08/...`
- Pins `UCP_*_SPEC_SHA256`, `UCP_*_SCHEMA_SHA256`,
  `UCP_REST_SCHEMA_SHA256`

---

## Gateway route

`POST /v1/http/ucp/checkouts/complete`
(`interop/crates/layerx-interop-gateway/src/server.rs`).

---

## Public types

`Capability`, `RestService`, `PaymentHandler`,
`MerchantProfile::layerx`, `PlatformProfile`,
`NegotiatedCapabilities::negotiate`, `UcpIdempotencyKey`,
`CheckoutStatus`, `CheckoutSubmission`, `OrderMetadata`,
`UcpPaymentIntent`, `UcpPlaneResult`, `ExecutedUcpPayment`,
`UcpPaymentPlane`, `UcpOrder`, `CheckoutOutcome`, `StoredOrder`,
`UcpAdapter::complete_checkout`, `UcpAdapter::read_order`,
`UcpError`, `ucp_adapter_descriptor()`, `interop_ucp()`.

Checkout completion is receipt-gated. An order read is not a
balance write.

[Interop](Interop.md) · [Home](Home.md)
