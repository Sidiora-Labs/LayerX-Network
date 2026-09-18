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

## Image, manifest and cluster configuration

`Dockerfile` builds the `layerx-interop-gateway` binary from the tracked
repository sources and runs it as the non-root 4020 user, the same identity the
hosted gateway image uses. `platform/hosted/interop/deployment.yaml` deploys it
into the `layerx-testnet` namespace beside the hosted gateway: it mounts its
server certificate, the internal CA, its outbound client identity, the gateway
authority client secret (receipt authority token and the sequencer pins), its
Redis credentials on the shared gateway keyspace, and the
`layerx-interop-runtime` secret holding `config.json` and `registry.json`.

`config.example.json` is the shape of the document `LAYERX_INTEROP_CONFIG`
selects. Every identity, key, digest, principal and merchant in it is an example
and must be replaced with authenticated operator configuration.

`platform/hosted/tests/beta-cluster.sh` renders that document during bring-up.
It derives the four vendored specification digests from `interop/specs/vendor`,
fixes each adapter's evidence policy, and takes everything the repository cannot
derive from the operator manifest named by `LAYERX_BETA_INTEROP_MANIFEST_FILE`:

    {
      "adapters": {
        "x402":     {"conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...},
        "ap2":      {"conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...},
        "ucp":      {"specification": ..., "version": ..., "conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...},
        "visa-tap": {"specification": ..., "version": ..., "conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...},
        "fiat":     {"specification": ..., "version": ..., "specification_sha256": ..., "conformance_suite": ..., "conformance_vectors": ..., "conformance_sha256": ...}
      },
      "transports": {"http": {...}, "mcp": {...}, "a2a": {...}},
      "x402_supported": {...}, "ap2_keys": [...], "ap2_assets": [...],
      "ucp_payment_handler": {...}, "visa_agents": [...], "visa_targets": [...],
      "fiat_providers": [...]
    }

Each transport entry declares `version`, `specification_sha256` and
`conformance_sha256`. The bring-up refuses to start when the variable is unset
and names every field the manifest fails to declare; there is no default trust
root and no adapter is silently skipped.
