# x402 vendored specification provenance

| Field | Value |
|---|---|
| Protocol | x402 v2 |
| Upstream repository | https://github.com/coinbase/x402 |
| Pinned commit | `7d5363a6d51750dc246041f2b0ed5819dd46a0d7` |
| Source URL | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/specs/x402-specification-v2.md |
| Retrieved | 2026-08-26 |
| File | `x402-specification-v2.md` |
| SHA-256 | `7d9be66cbcf51d3593e17ac51a623395f8ccb86fd3d76a27919419e4ce83efef` |

The document was fetched twice on the retrieval date and both fetches produced
identical bytes. The digest above is compiled into `layerx-x402` as
`X402_SPEC_SHA256` and is verified against this file by
`layerx-x402/tests/pinned_spec.rs`.

The upstream commit also contains per-scheme and per-transport documents
(`specs/schemes/`, `specs/transports-v2/`) that are referenced from the core
specification but are not vendored here; only the core specification document
above is content-pinned.
