# layerx-mirror

Publishes LayerX batch archives to Ethereum and Solana, and verifies those archives. Pure archives: commitments plus retrievable data. No vault, no portal, no custody.

Settlement stays on Paxeer. See the workspace index: [`../../README.md`](../../README.md).

| Binary | Role |
| --- | --- |
| `layerx-mirror-publisher` | `cargo run --bin layerx-mirror-publisher -- <config.json>` |
| `layerx-mirror-verify` | `cargo run --bin layerx-mirror-verify -- <config.json>` |

On-chain programs: `interop/contracts/ethereum-mirror/`, `interop/contracts/solana-mirror/`. Remote signer framing: [`../../deploy/mirror/signer-protocol.md`](../../deploy/mirror/signer-protocol.md).

From the monorepo root: `make interop-build`, `make interop-test`. Operator live targets: `make mirror-live`, `make mirror-verify-live`.
