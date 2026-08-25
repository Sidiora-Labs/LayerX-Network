# layerx-migrate

Ethereum and Solana source-chain verifiers for the `migration` interop adapter. This crate imports provenance against pinned source deployments. It does not write LayerX balances and it is not the C17 genesis cutover.

Operator contract: [`OPERATIONS.md`](OPERATIONS.md). Workspace index: [`../../README.md`](../../README.md).

LayerX genesis / cutover (different surface): [`spec/layerx-protocol/docs/migration.md`](../../../spec/layerx-protocol/docs/migration.md) and [`migrations/`](../../../migrations/README.md).

From the monorepo root: `make interop-test-migration`. The ignored live-testnet suite is `make interop-test-migration-testnets` (manual workflow, operator environment).
