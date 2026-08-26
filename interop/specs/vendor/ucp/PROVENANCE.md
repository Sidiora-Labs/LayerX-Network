# UCP vendored specification provenance

All documents were retrieved on 2026-08-26 from the published UCP revision
`2026-04-08` at https://ucp.dev. Each document was fetched twice on the
retrieval date and both fetches produced identical bytes. UCP publishes this
revision as rendered site content rather than a tagged archive; the public
source repository (https://github.com/Universal-Commerce-Protocol/ucp) was at
commit `19cd93cd29c632b306c8cac91a2ad173d07d1539` on the retrieval date. The
specification pages are served from GitHub Pages and are vendored as the exact
HTML bytes returned by the canonical trailing-slash URLs; the URL forms without
the trailing slash return a redirect stub, not the specification.

| File | Source URL | SHA-256 |
|---|---|---|
| `specification-checkout.html` | https://ucp.dev/2026-04-08/specification/checkout/ | `a579df10ae589a63f8f0d01e7b4f8e3183ba227025c692bb0385d92362b894b3` |
| `specification-order.html` | https://ucp.dev/2026-04-08/specification/order/ | `4463b524582cbdd5e5f1b84d5870e90ed8aef22a1c15420e48364e08d1d379ab` |
| `checkout.schema.json` | https://ucp.dev/2026-04-08/schemas/shopping/checkout.json | `b7d43000bdb1f845334af1e3e1a9a371758440dde166b5a03107bdacdd5948ac` |
| `order.schema.json` | https://ucp.dev/2026-04-08/schemas/shopping/order.json | `12e6221ac753a202ea3a2f2305e894b660ea81ebd1189bf29f88f48dd5d92eed` |
| `rest.openapi.json` | https://ucp.dev/2026-04-08/services/shopping/rest.openapi.json | `e50a1954414de233444f7262f6570646d941cbc1c9f32b876084db7f01e8997f` |

The digests above are compiled into `layerx-ucp` as
`UCP_CHECKOUT_SPEC_SHA256`, `UCP_ORDER_SPEC_SHA256`,
`UCP_CHECKOUT_SCHEMA_SHA256`, `UCP_ORDER_SCHEMA_SHA256` and
`UCP_REST_SCHEMA_SHA256`, and are verified against these files by
`layerx-ucp/tests/pinned_spec.rs`.
