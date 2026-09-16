# Fiat ramps

`layerx-fiat` is the interop-edge adapter for card, bank, and
real-time-payment **provider callbacks**
(`interop/crates/layerx-fiat/src/lib.rs`). The adapter id is `fiat`.
`spec/layerx-platform/spec.kvx` task 24.2 is **done**.

This is not the independent market-maker ramp toolkit. That lives
under `platform/ramps` and is documented on
[Market-maker ramps](MarketMakerRamps.md).

---

## Gateway routes

- `POST /v1/http/fiat/card/callbacks`
- `POST /v1/http/fiat/bank/callbacks`
- `POST /v1/http/fiat/rtp/callbacks`

---

## Public types

`FiatRail` (`Card`, `Bank`, `RealTimePayment`),
`EvidenceClass` (`Authorised`, `Clearing`, `Settled`, `Reversed`,
`Chargeback`), `ExternalId`, `TokenReference`, `ProviderEvidence`,
`VerifiedProviderFacts`, `ProviderVerifier`, `FiatIntentKind`,
`FiatIntent`, `FiatPlaneResult`, `FiatPlane`, `FiatJourneyState`,
`FiatAdapter` (`verify_evidence`, `apply`, `idempotency_key`),
`FiatError`, `fiat_adapter_descriptor()`,
`interop_fiat_adapters()`.

Callbacks carry token references only. PAN-shaped input is refused.
Credit, reversal, and chargeback apply only after
`ProviderVerifier` accepts the provider evidence and the LayerX
plane returns a receipt-gated result.

[Interop](Interop.md) · [Home](Home.md)
