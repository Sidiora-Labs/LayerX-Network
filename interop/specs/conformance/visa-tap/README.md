# Visa Trusted Agent Protocol Conformance Vectors

The first-party Visa TAP suite: every vector is an HTTP Message Signatures
credential presentation or a signed-target representation, together with the
verification outcome the production types must reach.

## Files

- `credential-verification.json` — full credential presentations and outcomes
- `target-canonicalization.json` — authority and path representations

## Vector Format

Each file holds a JSON array of records. Credential records carry the exact
wire strings of the presentation and the registry it is verified against:

- `name`: the case the vector covers
- `signing_key_seed`: the 32-byte Ed25519 seed the record is signed with
- `authority`, `path`: the covered target components
- `signed_authority`, `signed_path`: what the signature actually covers, so a
  tampered presentation can be expressed as data
- `signature_params`: the exact RFC 9421 `Signature-Input` parameters
- `registry_*`, `key_status`, `key_expires_at`: the registered agent key
- `clock_skew_seconds`: a number verifies through `verify_credential`, `null`
  verifies through `verify` and a shared nonce window
- `now`, `replays`: verification time and how many accepted deliveries precede
  this one
- `expect`: `verified` or the exact `TapError` variant
- `intent`: the intent a verified record must carry

Target records carry `authority`, `path`, `signature_input`, `signature`,
`accepted` and, when refused, `refusal`.

## Conformance

`interop/crates/layerx-visa-tap/tests/conformance.rs` reads these files with
`include_str!` and runs every record through the production `TapRequest`,
`SignatureInput`, `RegisteredAgentKey`, `NonceWindow` and `verify` types. The
Ed25519 signature itself is produced here from the record's own key seed,
because a signature is a live signer's output over the canonical signature base
and cannot be a literal; every other byte of the presentation is data.

One case in that file is not a vector and is not counted:
`binding_is_scoped_non_authoritative_and_success_requires_a_real_receipt`
drives the binding store and a canonical `LayerX` receipt rather than a
credential presentation.

`interop/deploy/gateway/render.py` derives the suite identifier, the vector
count and the SHA-256 the gateway configuration declares from these files.
