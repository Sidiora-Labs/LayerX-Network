# Upstream conformance suite survey

`ConformanceSuite` requires a suite identifier, a positive vector count and the
content digest of the suite the adapter passes. The interop gateway therefore
needs an imported conformance suite per adapter. This file records what each
upstream actually publishes at the commit this repository pins, so the suites
that cannot be vendored are named rather than invented.

The trees below were read from the GitHub tree API at the pinned commit on
2026-09-18 and searched for `conformance`, `vector`, `golden`, `testdata` and
`fixture` paths.

| Adapter | Upstream repository | Pinned commit | Conformance vectors published |
|---|---|---|---|
| x402 | https://github.com/coinbase/x402 | `7d5363a6d51750dc246041f2b0ed5819dd46a0d7` | none |
| AP2 | https://github.com/google-agentic-commerce/AP2 | `e1ea56db72a6385bce3e5c1112b3a56ce60acb43` | none |
| UCP | https://github.com/Universal-Commerce-Protocol/ucp | `19cd93cd29c632b306c8cac91a2ad173d07d1539` | none |
| Visa TAP | https://github.com/visa/trusted-agent-protocol | `16d59bdf3f8a542bc538d0962edbb80ea30a02af` | none |
| fiat | no upstream: `layerx-fiat` is this repository's own provider-callback adapter | — | — |

Findings per adapter:

- **x402** (3046 blobs): `specs/` carries the core specification, the scheme
  documents and the v2 transport bindings, all prose. The only vector-shaped
  paths are reference-implementation unit-test fixtures
  (`go/test/unit/permit2_fixtures_test.go`,
  `typescript/packages/core/test/mocks/generic/testDataBuilders.ts`,
  `typescript/packages/mechanisms/near/test/unit/fixtures/near.fixture.ts`).
  Those are tests of one implementation, not a published suite with expected
  outcomes, and they are not a conformance suite for another implementation to
  import.
- **AP2** (352 blobs): the repository publishes the specification document and
  `code/samples/` reference agents. No vector or conformance path exists.
- **UCP** (334 blobs): the repository publishes JSON Schemas
  (`source/schemas/**`), request/response scaffolds used by its own site build
  (`scripts/scaffolds/*.json`) and a schema-validation script. There is no
  suite of cases with expected outcomes.
- **Visa TAP** (71 blobs): the repository is a reference implementation
  (agent registry, merchant backend and frontend, CDN proxy) with no vector
  set. Its `LICENSE.md` is a pointer to the Visa Developer Center Terms of
  Use rather than a redistribution license, so beyond the already vendored
  `README.md` nothing further is vendored from it.
- **fiat**: the adapter implements LayerX's own provider-callback surface
  (`interop/crates/layerx-fiat/src/lib.rs`, documented in
  `docs/wiki/FiatRamps.md`). There is no upstream protocol body and therefore
  no upstream suite.

Consequence for the bring-up: the five adapter conformance triples and the
three transport conformance digests remain deployment inputs, named one
variable per adapter and per transport in
`interop/deploy/gateway/README.md`. Everything else the gateway configuration
needs — specification identifiers, versions and digests for all five adapters,
and the transport versions and specification digests — is derived from the
vendored documents at render time. No vector count or suite digest is
synthesised here or by the renderer.
