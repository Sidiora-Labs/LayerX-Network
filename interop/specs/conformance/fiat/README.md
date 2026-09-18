# Fiat Provider Callback Conformance Vectors

The first-party fiat suite: every vector is either an exact provider token
reference or one provider-evidence journey, together with the outcome the
production adapter must reach.

## Files

- `token-references.json` — the exact token bytes the card-data boundary
  accepts or refuses
- `journeys.json` — one provider-evidence journey per record

## Vector Format

Each file holds a JSON array of records. Token records carry `name`, `valid`,
either `token` (the literal bytes) or `token_hex` (for bytes no text can
express), the exact `refusal` for an invalid record, and an optional
`redaction_probe` a debug rendering must not leak.

Journey records carry:

- `name`, `principal`, `trace`, `token`, `evidence`, `now`
- `provider_facts`: the facts the certified provider returns — `provider`,
  `settlement`, `rail`, `evidence_class`, `amount`, `asset`, `destination`,
  `observed_at`, `hold_until`, and `fault` for a provider that refuses
- `plane`: `pending`, `refused` or `executed`; an executed plane carries the
  `receipt` fields (`signing_key_seed`, `sequence`, `amount`, `from`, `to`)
- `replay_at`: when set, the same journey is delivered twice and must return
  one outcome
- `expect`: the declared `state` (with `until` or `hold_until`) or the exact
  `refusal`

## Conformance

`interop/crates/layerx-fiat/tests/adapter.rs` reads these files with
`include_str!` and runs every record through the production `TokenReference`,
`VerifiedProviderFacts` and `FiatAdapter` types against a real `GatewayCore`,
so the suite the deployment pins is the suite the tests exercise. The canonical
receipt an executed journey settles against is signed here from the record's own
key seed, because a receipt signature is a live signer's output over canonical
bytes and cannot be a literal.

`interop/deploy/gateway/render.py` derives the suite identifier, the vector
count and the SHA-256 the gateway configuration declares from these files.
