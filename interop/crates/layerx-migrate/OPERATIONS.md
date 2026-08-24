# Migration source verifier operations

`EthereumVerifier` and `SolanaVerifier` are the production source boundaries used by `MigrationAdapter`. Construct them from `EthereumConfig` or `SolanaConfig`; both configuration types deserialize from JSON. The verifier must remain alive for verification and the subsequent history-cursor commit.

Each source and rollback-anchor quorum requires two to eight independent HTTPS JSON-RPC endpoints and a strict majority. Endpoint URLs must use DNS names and HTTPS. Each endpoint reads its bearer credential and DER trust anchor from operator-owned files. Repeating the same normalized host, port, and path is refused; using aliases backed by the same operator is still a deployment trust error and must be prevented by configuration review.

Ethereum configuration pins the chain ID, genesis block, custody contract, immutable runtime or explicit proxy implementation, runtime hashes at the source block, ABI selector, event topic, and the location of every custody field. Solana configuration pins the genesis hash, custody program, immutable ProgramData account and code hash, loader, custody and token authorities, account owners, instruction discriminator, account indices, byte offsets, and integer encoding. No default custody schema or deployment identifier exists.

The journal directory is local durable storage protected by an HMAC key file. Its head is additionally reconciled on every read and append against a strict-majority HTTPS authority. That authority must provide linearizable, durable implementations of:

- `layerx_getMigrationJournalHead([anchor_id])`, returning `{sequence,digest}`.
- `layerx_advanceMigrationJournalHead([{anchor_id,expected_sequence,expected_digest,sequence,digest}])`, performing an authenticated compare-and-swap and returning the committed `{sequence,digest}`.

The authority must never move a head backwards and must retain heads independently of the verifier host. Missing, divergent, rolled-back, or unauthenticated head state makes the verifier fail closed. Journal authentication keys, RPC bearer tokens, and authority credentials must be readable only by the service identity.

Account mapping requires a wallet-signed, bounded ownership claim and a deployment-specific `BindingReceiptPolicy`. Asset migration requires an exact custody claim and a deployment-specific `CustodyReceiptPolicy`. Both policies pin the independently trusted sequencer public key and verify the resulting protocol receipt, including authority, module and operation coordinates, exact balance effects, and an external-claim context commitment. The production plane receives only adapter-created execution requests with canonical idempotency keys and cannot substitute its own batch signer.

History pages are external provenance. They are prepared in the authenticated journal, stored through `ExternalHistorySink`, then committed through the same verifier. A sink must durably deduplicate by chain, native transaction identifier, address, asset, and kind before returning success. It must never translate an imported record into a LayerX activity or receipt.

The protected `migration-testnets` GitHub environment supplies exact deployed configuration, evidence envelopes, credentials, journal keys, and trust anchors to `.github/workflows/interop-migration-testnets.yml`. The workflow is manually dispatched because it reads live test networks and durable operator state. It exercises the production verifiers through `make interop-test-migration-testnets`.
