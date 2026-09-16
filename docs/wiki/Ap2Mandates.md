# AP2 mandates

`layerx-ap2` verifies AP2 v1 mandates (SD-JWT / JOSE), checks
constraints, and maps an execute path onto typed LayerX intents plus
receipt signing (`interop/crates/layerx-ap2/src/lib.rs`). The adapter
id is `ap2`. Pins are `AP2_SPEC_COMMIT` and `AP2_SPEC_SHA256`.
`spec/layerx-platform/spec.kvx` task 23.1 is **done**.

This page is that crate. x402 lives on
[x402 transport](X402Transport.md). Portable export of a mandate pair
is also used by [Portable receipt verifier](PortableVerifier.md)
external evidence.

---

## Gateway routes

HTTP only (`interop/crates/layerx-interop-gateway/src/server.rs`):

- `POST /v1/http/ap2/mandates/verify`
- `POST /v1/http/ap2/execute`

---

## Public types

| Module | Names |
| --- | --- |
| verify | `MandateMode`, `MandateUsage`, `VerificationContext`, `VerifiedMandates`, `MandateVerifier`, `MandateVerifier::verify` |
| jose | `KeyUse`, `ProtectedHeader`, `KeyResolver` |
| model | `Merchant`, `PaymentAmount`, `PaymentInstrument`, `Pisp` |
| adapter | `LayerXAssetBinding`, `AuthorizedPayment`, `LayerXIntentPlane`, `ExecutedPayment`, `PlaneOutcome`, `ReceiptSigner`, `PortableLayerXEvidence`, `SignedAp2Evidence`, `AdapterOutcome`, `Ap2Adapter`, `authorize_payment`, `Ap2Adapter::execute` |
| portable | `AP2_MANDATE_PAIR_MEDIA_TYPE`, `Ap2MandatePair`, `Ap2ExternalMandateVerifier` |
| error | `Ap2Error` |

`PortableLayerXEvidence` is the portable receipt type from
`layerx-portable`. `Ap2ExternalMandateVerifier` implements
`ExternalEvidenceVerifier`.

Descriptor: `ap2_adapter_descriptor()`. Codify anchor:
`interop_ap2()`.

Conformance vectors live under
`interop/crates/layerx-ap2/tests/vectors/` (amount-range, binding
mismatch, expired, invalid signature, autonomous line items).

[Interop](Interop.md) · [Home](Home.md)
