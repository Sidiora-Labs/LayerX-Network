# Interop gateway runtime additions

`runtime.env.example` lists the protocol scope and TAP clock inputs that the
interop service now requires. `LAYERX_INTEROP_MODULE_REGISTRY_FILE` must point
to the authoritative module registry mounted from the same core declaration as
the hosted gateway; `module-registry.example.json` documents its accepted
shape and is not a production registry.

The main file selected by `LAYERX_INTEROP_CONFIG` must include the two arrays
shown in `visa-trust.example.json`. Every `visa_agents` entry declares an
explicit `active` or `revoked` status. Every authenticated merchant principal
has exactly one canonical lowercase authority and canonical query-free path in
`visa_targets`. Missing targets, unknown keys, revoked keys, expired keys,
non-canonical targets, and duplicate principal targets fail closed at startup
or request admission.

The TAP skew is a server-owned deployment value in seconds, from zero through
300. It is never accepted from the public request. Production deployments must
replace every example identity, key, expiry, principal, module, and ordinal
with authenticated operator configuration.

Fiat provider callbacks carry an opaque `token_reference` beside a signed
evidence envelope. The evidence `facts` object must include
`token_reference_sha256`, the lowercase hexadecimal SHA-256 digest of those
exact token bytes. Providers sign the UTF-8 bytes
`LayerX/interop/fiat/provider-evidence/v1\0` followed immediately by the
compact JSON serialization of `facts`. The configured Ed25519 provider key
must verify that signature; evidence signed without the domain or for another
token is refused before any hold or protocol activity is admitted.
The callback does not accept an activity idempotency override. The service
derives the economic key from the authenticated provider, settlement, rail,
and evidence class, so retries converge and one settlement cannot be credited
again under a fresh caller-selected key.

Every AP2 asset binding declares one deployment-owned `audience`; all currency
bindings use canonical lowercase principal digests and are bounded and unique
per principal and currency. AP2 request bodies carry the signed nonce but
cannot override time, clock skew, audience, currency exponent, or activity
idempotency. The service verifies against its own clock with zero skew and
tries only the deployment-owned audience and exponent pairs for the
authenticated principal. Exactly one verified pair whose currency matches its
binding is required. The hosted execution key is derived from the canonical
authenticated principal and both verified mandate references.

## Image, configuration and cluster bring-up

`Dockerfile` builds the `layerx-interop-gateway` binary from the tracked
repository sources and runs it as the non-root 4020 user, the same identity the
hosted gateway image uses. `platform/hosted/interop/deployment.yaml` deploys it
into the `layerx-testnet` namespace beside the hosted gateway: it mounts its
server certificate, the internal CA, its outbound client identity, the gateway
authority client secret (receipt authority token and the sequencer pins), its
Redis credentials on the shared gateway keyspace, and the
`layerx-interop-runtime` secret holding `config.json` and `registry.json`.

`config.example.json` is the shape of the document `LAYERX_INTEROP_CONFIG`
selects. Its derived fields, including all eight conformance suites, are the
real ones this checkout renders; every key, principal and account in it is an
example. The `layerx-beta-*` identifiers mark
the trust roots the bring-up generates for this testnet's own test clients: they
are not authenticated external counterparties, and a deployment that faces real
ones replaces them with the variables below.

`render.py` produces that document and
`platform/hosted/tests/beta-cluster.sh` calls it during bring-up. The
deployment supplies variables only: there is no document to author, and
`render.py --check` refuses the bring-up up front, naming every variable that
is absent or malformed.

### Derived by `render.py`, never a deployment input

| Field | Source |
|---|---|
| `x402` specification, version `2.0.0`, digest | `interop/specs/vendor/x402/x402-specification-v2.md` |
| `ap2` specification, version `1.0.0`, digest | `interop/specs/vendor/ap2/specification.md` |
| `ucp` specification `ucp-checkout`, version `20260408`, digest | `interop/specs/vendor/ucp/specification-checkout.html` at the vendored `2026-04-08` revision |
| `visa-tap` specification, version `1`, digest | `interop/specs/vendor/visa-tap/README.md` |
| `fiat` specification `layerx-fiat-settlement`, version `1`, digest | `docs/wiki/FiatRamps.md`, the adapter's own surface description; there is no upstream |
| `http`, `mcp` and `a2a` binding version `2` and specification digests | `interop/specs/vendor/x402/transports/*.md`, the x402 v2 transport bindings vendored at the same pinned commit |
| `x402` suite `layerx-x402-conformance-v1`, its vector count and digest | `interop/specs/conformance/x402`, the vector files `interop/crates/layerx-x402/tests/vectors.rs` runs through the production types |
| `ap2` suite `layerx-ap2-conformance-v1`, its vector count and digest | `interop/specs/conformance/ap2`, the vector files `interop/crates/layerx-ap2/tests/mandates.rs` runs through `MandateVerifier` |
| `ucp` suite `layerx-ucp-conformance-v1`, its vector count and digest | `interop/specs/conformance/ucp`, the vector files `interop/crates/layerx-ucp/tests/conformance_vectors.rs` runs through the production checkout types |
| `visa-tap` suite `layerx-visa-tap-conformance-v1`, its vector count and digest | `interop/specs/conformance/visa-tap`, the vector files `interop/crates/layerx-visa-tap/tests/conformance.rs` runs through `TapRequest` and credential verification |
| `fiat` suite `layerx-fiat-conformance-v1`, its vector count and digest | `interop/specs/conformance/fiat`, the vector files `interop/crates/layerx-fiat/tests/adapter.rs` runs through `TokenReference` and `FiatAdapter` |
| `http`, `mcp` and `a2a` binding conformance digests | `interop/specs/conformance/transport-{http,mcp,a2a}`, the role-message vector files `interop/crates/layerx-x402/tests/transports.rs` runs through the production encoders and decoders |
| every adapter's `evidence_policy` | the policy the service already requires per adapter |
| `x402_supported` | this cluster's own facilitator declaration: the CAIP-2 form of the network the deployment serves, the `exact` scheme, and the generated sequencer identity as its signer |
| `ucp_payment_handler` | the `layerx-ucp-handler` declaration of the vendored UCP revision |

A first-party suite is derived from the files the adapter's own tests read, so
the pinned suite is the exercised suite: the identifier is
`layerx-<adapter>-conformance-v1`, the count is the number of vector records
under `interop/specs/conformance/<adapter>` (a transport binding's suite lives
under `transport-<binding>` and contributes its digest only), and the digest
covers each file's path and bytes. Editing, adding or removing a vector changes
both, and `--self-test` asserts every vector file is still `include_str!`-ed by
the test that owns it.

All eight suites are first party. A few cases cannot be expressed as data
because they exercise a live signer or the protocol's own encoders, so they stay
in Rust and are not counted in any suite: the fault-injected settlement cases in
`interop/crates/layerx-x402/tests/transports.rs`, which drive a live sequencer
signer and the canonical receipt encoder; the credential-binding case
`binding_is_scoped_non_authoritative_and_success_requires_a_real_receipt` in
`interop/crates/layerx-visa-tap/tests/conformance.rs`, which drives the binding
store and a canonical receipt; and the Codify anchor and vendored-revision
checks in `interop/crates/layerx-ucp/tests/conformance_vectors.rs`, which assert
against constants compiled into the crate. Where a vector's own signature is a
live signer's output — the Visa TAP presentations and the fiat settlement
receipts — the case shape, keys and expected outcome are data and only the
signing happens in Rust.

`x402_supported` and `ucp_payment_handler` are in-cluster counterparties, so
they default to the cluster's own material. `LAYERX_BETA_INTEROP_X402_SUPPORTED`
and `LAYERX_BETA_INTEROP_UCP_PAYMENT_HANDLER` hold a JSON document each and
replace those defaults when a deployment fronts a different facilitator or
payment handler.

### Generated beta trust roots

The AP2 issuer keys, the AP2 asset binding, the Visa TAP agent and merchant
target and the fiat provider callback key are counterparty credentials. On a
private testnet the counterparties are the cluster's own test clients, so
`secrets_generate` in `platform/hosted/tests/beta-cluster.sh` generates them —
three uncompressed SEC1 P-256 mandate keys, one ed25519 TAP agent key and one
ed25519 fiat provider key — and writes their public halves with the cluster
facts they bind to (the smoke client's principal digest and accounts, the node
asset, the interop service audience) to `$SECRETS_DIR/interop-beta-roots.json`.
`render.py --beta-roots-file` builds the pins from that material.

Every generated identifier says so: `layerx-beta-<use-case>-key`,
`layerx-beta-tap-key-1`, `layerx-beta-trusted-agent`, `layerx-beta-merchant`
and `layerx-beta-fiat-provider`. The rendered configuration carries no field
for provenance — the service refuses unknown fields — so the render prints the
roots it generated and the bring-up logs that line. These roots trust nothing
outside the cluster: they authenticate the testnet's own clients only.

### Deployment variables

No conformance variable is required. All eight suites are derived from this
checkout, so a bring-up that imports no external suite declares none of them.
The trust roots are still refused by name when the cluster can neither derive
them nor was given generated beta material.

Optional — each overrides a value this checkout or this cluster already
produces:

| Variable | Value | How to produce it |
|---|---|---|
| `LAYERX_BETA_INTEROP_CONFORMANCE_X402` | `<suite-identifier>,<vector-count>,<suite-sha256>` | overrides the first-party x402 suite with an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_AP2` | same form | overrides the first-party AP2 suite with an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_UCP` | same form | overrides the first-party UCP suite with an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_VISA_TAP` | same form | overrides the first-party Visa TAP suite with an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_FIAT` | same form | overrides the first-party fiat provider-callback suite with an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_HTTP` | `<suite-sha256>` | overrides the first-party HTTP transport suite digest with the SHA-256 of an imported one |
| `LAYERX_BETA_INTEROP_CONFORMANCE_MCP` | `<suite-sha256>` | as above for MCP |
| `LAYERX_BETA_INTEROP_CONFORMANCE_A2A` | `<suite-sha256>` | as above for A2A |
| `LAYERX_BETA_INTEROP_AP2_KEYS` | JSON array | the mandate issuer keys the AP2 credential provider publishes |
| `LAYERX_BETA_INTEROP_AP2_ASSETS` | JSON array | one binding per principal and currency, from the merchant agreement and the asset the deployment settles in |
| `LAYERX_BETA_INTEROP_VISA_AGENTS` | JSON array | the trusted-agent keys the Visa TAP registry publishes |
| `LAYERX_BETA_INTEROP_VISA_TARGETS` | JSON array | the merchant authority and path each principal is authorised for |
| `LAYERX_BETA_INTEROP_FIAT_PROVIDERS` | JSON array | the ed25519 callback key of each card, bank or RTP provider under contract |
| `LAYERX_BETA_INTEROP_X402_SUPPORTED` | JSON object | the facilitator declaration of a different x402 facilitator |
| `LAYERX_BETA_INTEROP_UCP_PAYMENT_HANDLER` | JSON object | the declaration of a different UCP payment handler |
| `LAYERX_BETA_INTEROP_MANIFEST_FILE` | path to a JSON document | overrides any rendered field, field by field |

No upstream publishes a conformance suite for UCP, Visa TAP, the fiat provider
callbacks or the x402 transport bindings — `interop/specs/vendor/CONFORMANCE.md`
records the tree each protocol publishes at its pinned commit and what was found
there — so this repository carries its own vectors for each of them and derives
the pin from the files the adapter's tests read. A declared variable still wins,
and the checks that make a pin mean something are unchanged: the renderer
refuses a suite with no vectors, a zero digest, a malformed pin and an
out-of-charset identifier rather than inventing any of them, and
`ConformanceSuite` keeps meaning a suite that actually ran. Declaring a trust
root replaces the generated beta root with a real external counterparty; the
render still refuses a root that is empty or malformed, and refuses by name
when no beta material is supplied either.

`LAYERX_BETA_INTEROP_MANIFEST_FILE` stays available for a deployment that
keeps its pins in one document. It is applied last and wins field by field, in
the shape:

    {
      "adapters": {"x402": {"conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...}, ...},
      "transports": {"http": {"conformance_sha256": ...}, ...},
      "x402_supported": {...}, "ap2_keys": [...], "ap2_assets": [...],
      "ucp_payment_handler": {...}, "visa_agents": [...], "visa_targets": [...],
      "fiat_providers": [...]
    }

Every field it sets is validated exactly as a variable is, and an unknown
adapter, transport or field is refused rather than ignored.

`python3 interop/deploy/gateway/render.py --self-test` exercises the render
against the vendored documents and vectors in this checkout: the refusal list
when nothing is declared, which names no conformance variable at all, the
derived digests against the provenance records, each of the eight first-party
suites against its vector files and the test that `include_str!`-s them
(including that editing one byte of one vector changes the digest), the
in-cluster defaults, the generated beta roots and the refusals for incomplete
key material, the field-by-field override, the variable overrides for an adapter
and a transport suite, and the refusals for an empty suite, a zero digest, a
malformed pin and an out-of-charset suite identifier.
