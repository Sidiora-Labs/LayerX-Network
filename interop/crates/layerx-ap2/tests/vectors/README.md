# AP2 Golden Vectors

This directory contains golden vectors for AP2 mandate verification conformance testing.

## Structure

- `direct/` - Direct mandate flows (closed checkout + closed payment)
- `autonomous/` - Autonomous mandate flows (open checkout + open payment → closed)
- `constraints/` - Constraint validation scenarios
- `refusals/` - Expected failure cases

## Vector Format

Each vector is a JSON file containing:
- `description`: Human-readable test case description
- `generator`: Deterministic builder in `tests/mandates.rs`
- fixed public test-key scalars or the semantic input/mutation for that case
- `verification_context`: Context parameters (time, nonce, audience, etc.)
- `expected_outcome`: "success" or error variant name
- `expected_values`: For success cases, expected parsed values

## Conformance

These vectors target the AP2 specification version pinned at:
- Commit: e1ea56db72a6385bce3e5c1112b3a56ce60acb43
- SHA-256 of docs/ap2/specification.md: 32c3be5011f481d2760e56e7b9935b0842c3da0d5f7d7b8a68402a599f1e6aa3

## Generation

The JSON records are reviewable vector specifications. `tests/mandates.rs`
materializes them with fixed, valid P-256 issuer, merchant, and agent keys and
passes the resulting JWS and SD-JWT presentations through the production
`MandateVerifier`. Refusal vectors minimally alter or re-sign an authentic
presentation so failures occur at the intended cryptographic, binding, time,
or constraint boundary rather than at preliminary parsing.
