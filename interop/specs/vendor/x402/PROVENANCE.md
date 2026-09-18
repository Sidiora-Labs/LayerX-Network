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

## Vendored v2 transport bindings

The same upstream commit publishes the three transport bindings the interop
gateway exposes. They are vendored under `transports/` and each document
declares `x402Version: 2`, so the pinned transport binding revision is `2`.

| File | Source URL | SHA-256 |
|---|---|---|
| `transports/http.md` | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/specs/transports-v2/http.md | `4f0298aaa23ac75de0eb49b1e96e6e67a5b910d7527b3a71c63e426b3bf5bdfb` |
| `transports/mcp.md` | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/specs/transports-v2/mcp.md | `4f1e0bb50c60e3fe384142f589cd6323b8a9ec4432335ef29053d1220f9e2146` |
| `transports/a2a.md` | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/specs/transports-v2/a2a.md | `bbf078363a2fc4eeeb8f6e9b66cf36033f5acfc3984c0e8f427143368979d878` |

Each transport document was fetched twice on 2026-09-18 and both fetches
produced identical bytes. `platform/hosted/tests/beta-cluster.sh` derives the
`http`, `mcp` and `a2a` transport specification digests from these files at
render time, so they are not deployment inputs.

## License

The upstream repository is published under the Apache License 2.0. Its
`LICENSE` and `NOTICE` at the pinned commit are vendored beside the documents
as required by section 4 of that license.

| File | Source URL | SHA-256 |
|---|---|---|
| `LICENSE` | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/LICENSE | `50e6751797c50dedd75ef1b8a0d9e42f5f8472e9fbce91f34718e9f97b0c780a` |
| `NOTICE` | https://raw.githubusercontent.com/coinbase/x402/7d5363a6d51750dc246041f2b0ed5819dd46a0d7/NOTICE | `0b8a03260dc87d976ea6f24d7ff1fb3f8f361cff317f9dd0da0b8d96936eabf6` |

The upstream commit also contains per-scheme documents (`specs/schemes/`) that
are referenced from the core specification but are not vendored here.
