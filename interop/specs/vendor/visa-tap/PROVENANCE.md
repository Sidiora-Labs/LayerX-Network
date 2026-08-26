# Visa Trusted Agent Protocol vendored document provenance

| Field | Value |
|---|---|
| Protocol | Visa Trusted Agent Protocol (TAP) |
| Upstream repository | https://github.com/visa/trusted-agent-protocol |
| Pinned commit | `16d59bdf3f8a542bc538d0962edbb80ea30a02af` |
| Source URL | https://raw.githubusercontent.com/visa/trusted-agent-protocol/16d59bdf3f8a542bc538d0962edbb80ea30a02af/README.md |
| Retrieved | 2026-08-26 |
| File | `README.md` |
| SHA-256 | `5f5fbaef32d575d1f83a0a2c8051338c37f7224cd6afd464d638b8f2863cada5` |

The document was fetched twice on the retrieval date and both fetches produced
identical bytes. Visa publishes no standalone specification document for TAP;
the pinned repository is Visa's reference implementation, and its `README.md`
is the published protocol description at that revision (message signature
profile, key registry contract, and credential tags). The digest above is
compiled into `layerx-visa-tap` as `VISA_TAP_SPEC_SHA256` and is verified
against this file by `layerx-visa-tap/tests/pinned_spec.rs`.
