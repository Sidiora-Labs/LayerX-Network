# Visa TAP

`layerx-visa-tap` implements Visa Trusted Agent Protocol verification:
RFC 9421-style `sig2` signatures, a trusted-agent registry, nonce
replay, and browse/pay tags `agent-browser-auth` /
`agent-payer-auth` (`interop/crates/layerx-visa-tap/src/lib.rs`).
Binding to a LayerX agent is non-authoritative. Execute hands off a
typed intent. The adapter id is `visa-tap`.
`spec/layerx-platform/spec.kvx` task 23.3 is **done**.

---

## Gateway routes

- `POST /v1/http/visa-tap/intents/verify`
- `POST /v1/http/visa-tap/intents/execute`

---

## Public types

`AgentIntent`, `TapAlgorithm`, `SignatureInput`, `TapRequest`,
`KeyStatus`, `AgentPublicKey`, `RegisteredAgentKey`,
`TrustedAgentRegistry`, `NonceWindow`, `VerifiedTrustedAgent`,
`TrustedCommerceIntent`, `LayerXIntentAuthority`,
`MerchantOperationResult`, `TapVerifier::verify_credential`,
`TapVerifier::verify`, `CredentialBinding`,
`CredentialBindingStore`, `bind_verified_agent`,
`verified_agent_binding`, `prepare_trusted_intent`,
`MerchantCredentialStatus`, `TapError`, `canonical_tap_authority`,
`canonical_tap_path`.

Pins: `VISA_TAP_SPEC_COMMIT`, `VISA_TAP_SPEC_SHA256`,
`MAX_CLOCK_SKEW_SECONDS`. Descriptor:
`visa_tap_adapter_descriptor()`. Codify anchor:
`interop_visa_tap()`.

[Interop](Interop.md) · [Home](Home.md)
