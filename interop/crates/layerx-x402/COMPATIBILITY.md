# x402 compatibility matrix

The adapter is pinned to x402 v2 revision `7d5363a6d51750dc246041f2b0ed5819dd46a0d7`. Buyer, seller and facilitator core values are transported without semantic changes across HTTP, MCP and A2A. HTTP uses the normative `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE` and `PAYMENT-RESPONSE` headers for payment signals; facilitator endpoint messages are JSON bodies on all transports.

| Transport | Buyer | Seller | Facilitator | Local matrix |
|---|---:|---:|---:|---:|
| HTTP | yes | yes | yes | `tests/transports.rs` |
| MCP | yes | yes | yes | `tests/transports.rs` |
| A2A | yes | yes | yes | `tests/transports.rs` |

The local matrix proves strict encode/decode parity, validation and transport-independent settlement identity. It does not claim upstream reference-implementation conformance or live-service settlement. Those results may be added only from the pinned upstream vector corpus and an independently operated service; neither is embedded or simulated here.

## Facilitator conformance results

The facilitator carries the standard `/verify`, `/settle` and `/supported` messages as JSON bodies on HTTP, MCP and A2A. Verification is a read-only translation; a settlement reports success only after the gateway verifies the canonical LayerX receipt returned by the plane authority. The results below are produced by the crate's own conformance and fault-injection suites and are re-run on every change by the `Interop x402 transport matrix` GitHub Actions workflow (`.github/workflows/interop-x402.yml`, `make interop-test-x402`).

| Facilitator behaviour | Result | Evidence |
|---|---:|---|
| `/verify` read-only translation, never state-changing | pass | `tests/transports.rs`, `src/facilitator.rs` |
| `/settle` success requires a gateway-verified LayerX receipt | pass | `tests/transports.rs::confirmed_settlement_records_exactly_one_receipt_verified_effect` |
| Settlement identity is transport-independent and step-separated | pass | `tests/transports.rs::settlement_identity_is_transport_independent_and_step_separated` |
| One economic effect across HTTP, MCP and A2A delivery | pass | `tests/transports.rs::transport_independent_identity_settles_once_across_http_mcp_and_a2a` |
| Duplicate delivery does not double-charge | pass | `tests/transports.rs::duplicate_delivery_of_the_same_settlement_does_not_double_charge` |
| Recovery after a crashed/pending settlement, exactly-once effect | pass | `tests/transports.rs::settlement_recovers_after_a_crash_without_a_second_economic_effect` |
| A swapped receipt for a settled identity is refused | pass | `tests/transports.rs::a_swapped_receipt_for_a_settled_identity_is_refused` |
| Wire vectors: payment-required, payload, settlement round-trips | pass | `tests/vectors.rs` |

Each "pass" is a local, offline result over real code paths, real types and a real sequencer-signed receipt. It attests exactly-once economic effect under fault injection and strict wire conformance; it does not attest interoperability against a third-party facilitator deployment or the upstream reference vector corpus, which remain a human qualification step.
