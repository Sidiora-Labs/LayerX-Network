# AP2

`layerx-ap2` is the interoperability adapter for AP2 checkout and payment
mandates (`interop/crates/layerx-ap2`). The adapter id is `ap2`. The pinned
upstream revision is `e1ea56db72a6385bce3e5c1112b3a56ce60acb43`, compiled
from `interop/specs/vendor/ap2/specification.md`
(`interop/crates/layerx-ap2/src/lib.rs`).

The platform specification (`spec/layerx-platform/spec.kvx`, requirement 32
`ac_3`) requires the gateway to verify mandate signatures and constraints
before acting, map authorised mandates to typed intents, and refuse any
mandate whose constraints the resulting LayerX activity cannot honour.

## What the crate exposes

The library exports mandate verification (`MandateVerifier`,
`VerificationContext`, `VerifiedMandates`), adapter types
(`Ap2Adapter`, `AuthorizedPayment`, `ExecutedPayment`), and a portable
mandate pair (`Ap2MandatePair`, `AP2_MANDATE_PAIR_MEDIA_TYPE`).
`ap2_adapter_descriptor` builds the gateway descriptor for AP2 mandate
schema version `1.0.0` against a caller-supplied conformance suite. No
synthetic vector digest is embedded.

Adapters translate. They do not write balances
([Interop](index.md)).

## What this page does not document

The x402 wiki page states that it does not cover AP2. This crate does not
declare public HTTP routes of its own in `src/lib.rs`. Hosted product URLs
for an AP2 checkout endpoint are not named in the copied wiki set. If a
deployment exposes AP2 through the interoperability gateway, that wiring is
an operator configuration of `interop/crates/layerx-interop-service`, not a
public testnet hostname documented here.

## Status

The adapter crate, pinned specification document, and tests under
`interop/crates/layerx-ap2/tests/` are in this tree. Platform interop
conformance and beta executed-evidence gates remain qualification work
(`spec/layerx-platform` task "Run the interop conformance matrix";
`spec/layerx-beta`).
