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
selects. Its derived fields are the real ones this checkout renders; every
conformance digest, identity, principal and merchant in it is an example and
must be replaced with authenticated deployment configuration.

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
| every adapter's `evidence_policy` | the policy the service already requires per adapter |
| `x402_supported` | this cluster's own facilitator declaration: the CAIP-2 form of the network the deployment serves, the `exact` scheme, and the generated sequencer identity as its signer |
| `ucp_payment_handler` | the `layerx-ucp-handler` declaration of the vendored UCP revision |

`x402_supported` and `ucp_payment_handler` are in-cluster counterparties, so
they default to the cluster's own material. `LAYERX_BETA_INTEROP_X402_SUPPORTED`
and `LAYERX_BETA_INTEROP_UCP_PAYMENT_HANDLER` hold a JSON document each and
replace those defaults when a deployment fronts a different facilitator or
payment handler.

### Deployment variables

| Variable | Value | How to produce it |
|---|---|---|
| `LAYERX_BETA_INTEROP_CONFORMANCE_X402` | `<suite-identifier>,<vector-count>,<suite-sha256>` | run the x402 conformance suite the deployment imported, then name it, count its vectors and take the SHA-256 of the suite content |
| `LAYERX_BETA_INTEROP_CONFORMANCE_AP2` | same form | as above for AP2 |
| `LAYERX_BETA_INTEROP_CONFORMANCE_UCP` | same form | as above for UCP |
| `LAYERX_BETA_INTEROP_CONFORMANCE_VISA_TAP` | same form | as above for Visa TAP |
| `LAYERX_BETA_INTEROP_CONFORMANCE_FIAT` | same form | as above for the fiat provider-callback suite |
| `LAYERX_BETA_INTEROP_CONFORMANCE_HTTP` | `<suite-sha256>` | SHA-256 of the imported HTTP transport conformance suite |
| `LAYERX_BETA_INTEROP_CONFORMANCE_MCP` | `<suite-sha256>` | as above for MCP |
| `LAYERX_BETA_INTEROP_CONFORMANCE_A2A` | `<suite-sha256>` | as above for A2A |
| `LAYERX_BETA_INTEROP_AP2_KEYS` | JSON array | the mandate issuer keys the AP2 credential provider publishes |
| `LAYERX_BETA_INTEROP_AP2_ASSETS` | JSON array | one binding per principal and currency, from the merchant agreement and the asset the deployment settles in |
| `LAYERX_BETA_INTEROP_VISA_AGENTS` | JSON array | the trusted-agent keys the Visa TAP registry publishes |
| `LAYERX_BETA_INTEROP_VISA_TARGETS` | JSON array | the merchant authority and path each principal is authorised for |
| `LAYERX_BETA_INTEROP_FIAT_PROVIDERS` | JSON array | the ed25519 callback key of each card, bank or RTP provider under contract |
| `LAYERX_BETA_INTEROP_MANIFEST_FILE` | path to a JSON document | optional; overrides any rendered field, field by field |

The conformance suites are deployment inputs because no upstream publishes one:
`interop/specs/vendor/CONFORMANCE.md` records the tree each protocol publishes
at its pinned commit and what was found there. The renderer refuses a suite
with no vectors and a zero digest rather than inventing either, so
`ConformanceSuite` keeps meaning a suite that actually ran. The five trust
roots above are counterparty credentials this cluster cannot generate for
itself; the bring-up names each missing one instead of skipping the adapter.

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
against the vendored documents in this checkout: the refusal list when nothing
is declared, the derived digests against the provenance records, the in-cluster
defaults, the field-by-field override, and the refusals for an empty suite, a
zero digest and an out-of-charset suite identifier.
