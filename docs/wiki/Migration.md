# Ethereum and Solana migration

`layerx-migrate` maps external accounts, credits assets, and imports
external history through `SourceVerifier` + `MigrationPlane` +
gateway idempotency (`interop/crates/layerx-migrate/src/lib.rs`).
The adapter id is `migration`.

`spec/layerx-platform/spec.kvx` task 24.1 is **implemented**. The
spec `reality` field records that production RPC clients are
missing; the verifiers are heavily tested. This page does not claim
those clients are finished.

[Programs porting](Porting.md) is a different crate and topic.

---

## Public types

`SourceChain`, `ExternalAddress`, `SourceTransaction`,
`SourceEvidence`, `VerifiedOwnership`, `VerifiedAssetFinality`,
`SourceVerifier`, `ExternalHistoryKind`, `ExternalHistoryRecord`,
`ExternalProvenance`, `VerifiedHistoryPage`, `ExternalHistorySink`,
`BindingExecution`, `CustodyExecution`, `MigrationPlaneResult`,
`MigrationPlane`, `BindingReceiptPolicy`, `CustodyReceiptPolicy`,
`MigrationState`, `MigrationAdapter` (`map_account`,
`migrate_asset`, `import_history`), `MigrationError`,
`migration_adapter_descriptor()`, `interop_migrate_ethereum()`,
`interop_migrate_solana()`, `JournalConfig`, `RpcEndpointConfig`,
`RpcQuorumConfig`.

Chain modules: `EthereumVerifier`, `EthereumConfig`,
`ethereum_claim_digest`; `SolanaVerifier`, `SolanaConfig`,
`solana_claim_digest`.

Domain strings in the crate:

- `LayerX/interop/migration/request/v1`
- `LayerX/interop/migration/idempotency/v1`
- `LayerX/interop/migration/custody-context/v1`
- `LayerX/interop/migration/binding-context/v1`

Operator notes: `interop/crates/layerx-migrate/README.md` and
`OPERATIONS.md`.

The adapter is registered as `migration` on the gateway core. It is
not in the hosted service's `REQUIRED_ADAPTERS` list
(`x402`, `ap2`, `ucp`, `visa-tap`, `fiat` only).

[Interop](Interop.md) · [Home](Home.md)
